//! Windows 进程树实现：Job Object。Kill-on-close so Maven/npm child JVMs die with the job.
//! 1.4 由 `job.rs` 整体迁入，行为与错误码不变（Windows 零回归是硬约束）。

use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::process::{Child, Command};

use windows::Win32::Foundation::{CloseHandle, HANDLE};
use windows::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JobObjectBasicProcessIdList,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
};

use crate::error::{Error, ErrorCode, Result};
use crate::proc::ProcessTree;

const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 任意 pid 是否存活（1.5 工作区锁 stale 判定专用；只读探测，不发信号、不结束进程）。
/// OpenProcess 被拒（ERROR_ACCESS_DENIED，如受保护进程）视为存活——
/// 与 Unix 侧 EPERM 同口径；仅「参数无效」类失败判为不存在。
/// 打开成功还需 GetExitCodeProcess 排除「已退出但句柄未关」。
pub fn pid_alive(pid: u32) -> bool {
    use windows::Win32::Foundation::{CloseHandle, E_ACCESSDENIED, STILL_ACTIVE};
    use windows::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    if pid == 0 {
        return false;
    }
    unsafe {
        match OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            Err(e) => e.code() == E_ACCESSDENIED,
            Ok(handle) => {
                let mut exit_code: u32 = 0;
                let ok = GetExitCodeProcess(handle, &mut exit_code);
                let _ = CloseHandle(handle);
                ok.is_ok() && exit_code == STILL_ACTIVE.0 as u32
            }
        }
    }
}

#[link(name = "ntdll")]
extern "system" {
    fn NtResumeProcess(process: HANDLE) -> i32;
}

pub struct WindowsJob {
    handle: HANDLE,
}

// Kernel handle; all access is serialized by Engine's mutex.
unsafe impl Send for WindowsJob {}
unsafe impl Sync for WindowsJob {}

impl WindowsJob {
    pub fn create() -> Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| {
                Error::new(ErrorCode::JobCreate, format!("CreateJobObject 失败: {e}"))
            })?;
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            SetInformationJobObject(
                handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| Error::new(ErrorCode::JobCreate, format!("Job 限制设置失败: {e}")))?;
            Ok(Self { handle })
        }
    }

    pub fn assign_child(&self, child: &Child) -> Result<()> {
        let raw = child.as_raw_handle();
        let proc = HANDLE(raw);
        unsafe {
            AssignProcessToJobObject(self.handle, proc).map_err(|e| {
                Error::new(
                    ErrorCode::JobCreate,
                    format!("AssignProcessToJobObject 失败: {e}"),
                )
            })
        }
    }

    /// Resume after CREATE_SUSPENDED. Must run after assign.
    pub fn resume_child(&self, child: &Child) -> Result<()> {
        let proc = HANDLE(child.as_raw_handle());
        let st = unsafe { NtResumeProcess(proc) };
        if st < 0 {
            return Err(Error::new(
                ErrorCode::Spawn,
                format!("NtResumeProcess 失败 status={st:#x}"),
            ));
        }
        Ok(())
    }

    pub fn terminate(&self) -> Result<()> {
        unsafe {
            TerminateJobObject(self.handle, 1).map_err(|e| {
                Error::new(ErrorCode::JobKill, format!("TerminateJobObject 失败: {e}"))
            })
        }
    }

    /// Job 里全部存活 pid（查询失败返回空列表）。
    pub fn pids(&self) -> Vec<u32> {
        // 缓冲区给 64 个 pid 足够（每个服务一个根进程树）
        #[repr(C)]
        struct PidList {
            num_assigned: u32,
            num_full: u32,
            pids: [usize; 64],
        }
        let mut list = PidList {
            num_assigned: 0,
            num_full: 0,
            pids: [0; 64],
        };
        let ok = unsafe {
            QueryInformationJobObject(
                self.handle,
                JobObjectBasicProcessIdList,
                std::ptr::from_mut(&mut list).cast(),
                std::mem::size_of::<PidList>() as u32,
                None,
            )
        };
        if !ok.is_ok() {
            return Vec::new();
        }
        list.pids[..list.num_assigned.min(64) as usize]
            .iter()
            .map(|&p| p as u32)
            .collect()
    }

    /// Job 累计 CPU 时间（内核+用户，毫秒）。1.2 §9.3 指标用：
    /// 差分两次采样即得窗口 CPU。查询失败返回 None（不判服务异常）。
    pub fn total_cpu_ms(&self) -> Option<u64> {
        use windows::Win32::System::JobObjects::{
            JobObjectBasicAccountingInformation, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
        };
        unsafe {
            let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
            let ok = QueryInformationJobObject(
                self.handle,
                JobObjectBasicAccountingInformation,
                std::ptr::from_mut(&mut info).cast(),
                std::mem::size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>() as u32,
                None,
            );
            if ok.is_err() {
                return None;
            }
            let hundred_ns = info.TotalKernelTime.saturating_add(info.TotalUserTime);
            Some((hundred_ns / 10_000) as u64)
        }
    }

    /// Job 内进程工作集之和。单个进程查询失败跳过（部分可用），全部失败 None。
    pub fn working_set_bytes(&self) -> Option<u64> {
        use windows::Win32::System::ProcessStatus::{
            GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};
        let pids = self.pids();
        if pids.is_empty() {
            return None;
        }
        let mut any = false;
        let mut total: u64 = 0;
        for pid in pids {
            unsafe {
                let Ok(handle) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                    continue;
                };
                let mut pmc = PROCESS_MEMORY_COUNTERS::default();
                pmc.cb = std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32;
                if GetProcessMemoryInfo(handle, &mut pmc, pmc.cb).is_ok() {
                    total += pmc.WorkingSetSize as u64;
                    any = true;
                }
                let _ = CloseHandle(handle);
            }
        }
        any.then_some(total)
    }
}

impl Drop for WindowsJob {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

impl ProcessTree for WindowsJob {
    /// Spawn with CREATE_SUSPENDED, assign to job, then resume.
    /// Ceiling: NtResumeProcess is undocumented but is the practical way to resume
    /// a std::process::Child created suspended; upgrade is CreateProcess + hThread.
    fn spawn(&self, cmd: &mut Command) -> Result<Child> {
        cmd.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
        let child = cmd
            .spawn()
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("进程无法启动: {e}")))?;
        self.assign_child(&child)?;
        self.resume_child(&child)?;
        Ok(child)
    }

    fn terminate(&self) -> Result<()> {
        WindowsJob::terminate(self)
    }

    fn pids(&self) -> Vec<u32> {
        WindowsJob::pids(self)
    }

    fn total_cpu_ms(&self) -> Option<u64> {
        WindowsJob::total_cpu_ms(self)
    }

    fn working_set_bytes(&self) -> Option<u64> {
        WindowsJob::working_set_bytes(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn ping_dies_with_job() {
        let job = WindowsJob::create().expect("job");
        let mut cmd = Command::new("ping");
        cmd.args(["-t", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = job.spawn(&mut cmd).expect("spawn");
        std::thread::sleep(Duration::from_millis(200));
        assert!(!job.pids().is_empty(), "job 内应有存活 pid");
        job.terminate().expect("term");
        let st = child.wait().expect("wait");
        assert!(!st.success());
    }
}
