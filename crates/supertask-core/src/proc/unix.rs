//! Unix 进程树实现：每个引擎直系子进程独立进程组（1.4 §4.1）。
//! 停止 = `killpg` SIGTERM → 5s 宽限 → SIGKILL；Linux 直系子进程加 PDEATHSIG(SIGKILL) 兜底。
//! 局限（规格明示，不假装等价 Job Object）：macOS 引擎异常崩溃时孙进程可能残留；
//! 指标查询失败降级 None（`METRICS_UNAVAILABLE`），不影响状态机。

use std::io;
use std::process::{Child, Command};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use nix::sys::signal::Signal;

use crate::error::{Error, ErrorCode, Result};
use crate::proc::ProcessTree;

/// SIGTERM → SIGKILL 的宽限窗口（规格 §4.1：固定值，不受 `grace_secs` 影响）。
const KILL_GRACE: Duration = Duration::from_secs(5);

pub struct UnixProcessGroup {
    /// `setpgid(0,0)` 后 pgid == 直系子进程 pid；spawn 成功前为 None。
    pgid: Mutex<Option<i32>>,
}

impl UnixProcessGroup {
    pub fn new() -> Self {
        Self { pgid: Mutex::new(None) }
    }

    fn pgid(&self) -> Option<i32> {
        *self.pgid.lock().expect("pgid lock")
    }
}

impl ProcessTree for UnixProcessGroup {
    fn spawn(&self, cmd: &mut Command) -> Result<Child> {
        use std::os::unix::process::CommandExt;
        unsafe {
            cmd.pre_exec(|| {
                // 新进程组：子进程 pgid == 自身 pid，孙进程默认同组，killpg 覆盖整棵树
                if libc::setpgid(0, 0) != 0 {
                    return Err(io::Error::last_os_error());
                }
                #[cfg(target_os = "linux")]
                {
                    // PDEATHSIG：引擎异常崩溃时直系子进程随之 SIGKILL；
                    // 孙进程靠组终止尽力清场（规格明示「尽力」）
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL, 0, 0, 0) != 0 {
                        return Err(io::Error::last_os_error());
                    }
                }
                Ok(())
            });
        }
        let child = cmd
            .spawn()
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("进程无法启动: {e}")))?;
        *self.pgid.lock().expect("pgid lock") = Some(child.id() as i32);
        Ok(child)
    }

    fn terminate(&self) -> Result<()> {
        let Some(pgid) = self.pgid() else {
            return Ok(());
        };
        signal_group(pgid, Signal::SIGTERM);
        let deadline = Instant::now() + KILL_GRACE;
        while Instant::now() < deadline {
            if group_pids(pgid).is_empty() {
                return Ok(());
            }
            thread::sleep(Duration::from_millis(100));
        }
        // 超时仍存活 → 硬杀（语义对齐 JOB_KILL 口径）
        signal_group(pgid, Signal::SIGKILL);
        Ok(())
    }

    fn pids(&self) -> Vec<u32> {
        match self.pgid() {
            Some(pgid) => group_pids(pgid),
            None => Vec::new(),
        }
    }

    fn total_cpu_ms(&self) -> Option<u64> {
        let pgid = self.pgid()?;
        group_cpu_ms(pgid)
    }

    fn working_set_bytes(&self) -> Option<u64> {
        let pgid = self.pgid()?;
        group_memory_bytes(pgid)
    }
}

impl Drop for UnixProcessGroup {
    fn drop(&mut self) {
        // 兜住「drop 时组内仍有活进程」的正常退出路径；
        // Linux 异常崩溃另有 PDEATHSIG，macOS 异常崩溃残留为规格明示局限。
        if let Some(pgid) = self.pgid() {
            signal_group(pgid, Signal::SIGKILL);
        }
    }
}

fn signal_group(pgid: i32, sig: Signal) {
    use nix::sys::signal::killpg;
    use nix::unistd::Pid;
    // ESRCH = 组内已无进程，属正常
    let _ = killpg(Pid::from_raw(pgid), sig);
}

/// 组内存活 pid 列表。任一查询路径失败返回空列表（降级，不误判）。
#[cfg(target_os = "linux")]
fn group_pids(pgid: i32) -> Vec<u32> {
    let Ok(all) = procfs::process::all_processes() else {
        return Vec::new();
    };
    all.into_iter()
        .filter_map(|p| p.ok())
        .filter_map(|p| {
            let stat = p.stat().ok()?;
            (stat.pgrp == pgid).then_some(stat.pid as u32)
        })
        .collect()
}

#[cfg(not(target_os = "linux"))]
fn group_pids(pgid: i32) -> Vec<u32> {
    // macOS 无 /proc：`ps -axo pid=,pgid=` 系统自带，只读解析
    let Ok(out) = std::process::Command::new("ps")
        .args(["-axo", "pid=,pgid="])
        .output()
    else {
        return Vec::new();
    };
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines()
        .filter_map(|line| {
            let mut it = line.split_whitespace();
            let pid: u32 = it.next()?.parse().ok()?;
            let pg: i32 = it.next()?.parse().ok()?;
            (pg == pgid).then_some(pid)
        })
        .collect()
}

/// 组内累计 CPU（毫秒）。单进程查询失败跳过，全部失败返回 None。
#[cfg(target_os = "linux")]
fn group_cpu_ms(pgid: i32) -> Option<u64> {
    let pids = group_pids(pgid);
    if pids.is_empty() {
        return None;
    }
    let tps = procfs::ticks_per_second().max(1) as u64;
    let mut total_ticks = 0u64;
    let mut any = false;
    for pid in pids {
        if let Ok(stat) = procfs::process::Process::new(pid as i32).and_then(|p| p.stat()) {
            total_ticks += stat.utime + stat.stime;
            any = true;
        }
    }
    any.then(|| total_ticks * 1000 / tps)
}

#[cfg(not(target_os = "linux"))]
fn group_cpu_ms(pgid: i32) -> Option<u64> {
    let pids = group_pids(pgid);
    if pids.is_empty() {
        return None;
    }
    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let out = std::process::Command::new("ps")
        .args(["-o", "cputime=", "-p", &list])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut total = 0u64;
    let mut any = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // BSD ps 时间格式：`MM:SS.ss` 或 `HH:MM:SS`
        if let Some(ms) = parse_time_ms(t) {
            total += ms;
            any = true;
        }
    }
    any.then_some(total)
}

/// 组内内存（字节）。单进程查询失败跳过，全部失败返回 None。
#[cfg(target_os = "linux")]
fn group_memory_bytes(pgid: i32) -> Option<u64> {
    let pids = group_pids(pgid);
    if pids.is_empty() {
        return None;
    }
    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) }.max(1) as u64;
    let mut total = 0u64;
    let mut any = false;
    for pid in pids {
        if let Ok(stat) = procfs::process::Process::new(pid as i32).and_then(|p| p.stat()) {
            total += stat.rss * page;
            any = true;
        }
    }
    any.then_some(total)
}

#[cfg(not(target_os = "linux"))]
fn group_memory_bytes(pgid: i32) -> Option<u64> {
    let pids = group_pids(pgid);
    if pids.is_empty() {
        return None;
    }
    let list = pids.iter().map(|p| p.to_string()).collect::<Vec<_>>().join(",");
    let out = std::process::Command::new("ps")
        .args(["-o", "rss=", "-p", &list])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let mut total = 0u64;
    let mut any = false;
    for line in text.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        // RSS 单位 KB
        if let Ok(kb) = t.parse::<u64>() {
            total += kb * 1024;
            any = true;
        }
    }
    any.then_some(total)
}

/// `MM:SS.ss` / `HH:MM:SS` 通用解析，返回毫秒。
#[cfg(not(target_os = "linux"))]
fn parse_time_ms(s: &str) -> Option<u64> {
    let mut secs = 0f64;
    for part in s.split(':') {
        let v: f64 = part.trim().parse().ok()?;
        secs = secs * 60.0 + v;
    }
    Some((secs * 1000.0) as u64)
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::process::Stdio;

    #[test]
    fn sleep_tree_dies_with_group() {
        let tree = UnixProcessGroup::new();
        let mut cmd = Command::new("sleep");
        cmd.arg("300").stdout(Stdio::null()).stderr(Stdio::null());
        let mut child = tree.spawn(&mut cmd).expect("spawn");
        std::thread::sleep(Duration::from_millis(200));
        assert!(!tree.pids().is_empty(), "组内应有存活 pid");
        tree.terminate().expect("term");
        let st = child.wait().expect("wait");
        assert!(!st.success());
        assert!(tree.pids().is_empty(), "terminate 后组内不应有存活 pid");
    }
}
