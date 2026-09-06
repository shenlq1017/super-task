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

    /// 方向二·原地接管：把**已运行的外部进程**并入本 Job（kill-on-close 随即生效）。
    /// Windows 8+ 支持嵌套 Job：目标进程已在另一个 Job 内时分配失败并给出可诊断
    /// 错误（调用方回退到「外部实例仅监控」语义）。需要 PROCESS_SET_QUOTA +
    /// PROCESS_TERMINATE 访问权（同用户会话进程无需管理员）。
    /// 方向二·原地接管暂存 Job：与 `create` 同形但**不带 kill-on-close**。
    /// attach 失败/并发回退时直接 drop，不会误杀目标进程；提交成功后由
    /// `enable_kill_on_close` 转为正式监管语义。
    pub fn create_staging() -> Result<Self> {
        unsafe {
            let handle = CreateJobObjectW(None, None).map_err(|e| {
                Error::new(ErrorCode::JobCreate, format!("CreateJobObject 失败: {e}"))
            })?;
            Ok(Self { handle })
        }
    }

    /// 暂存 Job 转正：补上 kill-on-close 限制（随 Slot 提交原子生效）。
    /// Windows 允许在进程已并入后设置该限制，关闭句柄时同样终止整树。
    pub fn enable_kill_on_close(&self) -> Result<()> {
        self.set_kill_on_close(true)
    }

    /// 转正后提交失败的回滚：摘掉 kill-on-close 后再 drop，目标进程不受影响。
    /// 设置失败时调用方应 `mem::forget` 整个 Job（推迟到应用退出），绝不直接 drop。
    pub fn clear_kill_on_close(&self) -> Result<()> {
        self.set_kill_on_close(false)
    }

    fn set_kill_on_close(&self, kill: bool) -> Result<()> {
        unsafe {
            let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
            if kill {
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            }
            SetInformationJobObject(
                self.handle,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            )
            .map_err(|e| Error::new(ErrorCode::JobCreate, format!("Job 限制设置失败: {e}")))?;
            Ok(())
        }
    }

    pub fn attach_pid(&self, pid: u32) -> Result<()> {
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
        };
        unsafe {
            let proc =
                OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, false, pid).map_err(|e| {
                    Error::new(
                        ErrorCode::JobCreate,
                        format!("OpenProcess({pid}) 失败（受保护进程或权限不足）: {e}"),
                    )
                })?;
            let res = AssignProcessToJobObject(self.handle, proc);
            let _ = CloseHandle(proc);
            res.map_err(|e| {
                Error::new(
                    ErrorCode::JobCreate,
                    format!("AssignProcessToJobObject({pid}) 失败（进程可能已属于其他 Job）: {e}"),
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
                Some(self.handle),
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
                Some(self.handle),
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

    /// 方向二·原地接管：不带 CREATE_SUSPENDED 自行启动的外部进程（不经 job.spawn），
    /// 运行中 attach_pid 并入 Job → pids 可见 → terminate 整树终止。
    #[test]
    fn attach_running_external_pid_then_kill_tree() {
        // 外部进程：直接 Command::spawn（父进程不在任何 Job 内 → 子进程也不在）
        let mut external = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn external");
        std::thread::sleep(Duration::from_millis(200));
        let pid = external.id();
        let job = WindowsJob::create().expect("job");
        job.attach_pid(pid).expect("attach");
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            job.pids().contains(&pid),
            "attach 后 pid 应在 job 内: {:?}",
            job.pids()
        );
        job.terminate().expect("term");
        let st = external.wait().expect("wait");
        assert!(!st.success(), "terminate 后外部进程应已退出");
        // 已退出进程再 attach → 可诊断错误（不 panic）
        let job2 = WindowsJob::create().expect("job2");
        let e = job2.attach_pid(pid).expect_err("dead pid must fail");
        assert!(
            e.message().contains("OpenProcess") || e.message().contains("Assign"),
            "{}",
            e.message()
        );
    }

    /// 方向二·失败回滚安全：暂存 Job（无 kill-on-close）attach 后直接 drop，
    /// 目标进程必须存活——失败路径绝不误杀。转正则用 Job 兜底清理。
    #[test]
    fn staging_job_drop_does_not_kill_attached_pid() {
        let mut external = Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn external");
        std::thread::sleep(Duration::from_millis(200));
        let pid = external.id();
        {
            let staging = WindowsJob::create_staging().expect("staging");
            staging.attach_pid(pid).expect("attach to staging");
            // 模拟 attach 失败回退：暂存 Job 直接离开作用域
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            pid_alive(pid),
            "暂存 Job drop 后目标进程必须仍存活（无 kill-on-close）"
        );
        // 转正/回滚往返：enable 后 clear 再 drop，同样不杀进程
        {
            let staging = WindowsJob::create_staging().expect("staging");
            staging.attach_pid(pid).expect("re-attach to staging");
            staging.enable_kill_on_close().expect("enable");
            staging.clear_kill_on_close().expect("clear");
        }
        std::thread::sleep(Duration::from_millis(200));
        assert!(
            pid_alive(pid),
            "clear_kill_on_close 后 drop 不得误杀目标进程"
        );
        // 清理：正式 kill-on-close Job 接管后整树终止
        let killer = WindowsJob::create().expect("job");
        killer.attach_pid(pid).expect("attach to killer");
        killer.terminate().expect("term");
        let st = external.wait().expect("wait");
        assert!(!st.success(), "清理用 Job 应能终止目标进程");
    }
}
