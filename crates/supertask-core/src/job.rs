//! Windows Job Object. Kill-on-close so Maven/npm child JVMs die with the job.

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

const CREATE_SUSPENDED: u32 = 0x0000_0004;
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

#[link(name = "ntdll")]
extern "system" {
    fn NtResumeProcess(process: HANDLE) -> i32;
}

pub struct Job {
    handle: HANDLE,
}

// Kernel handle; all access is serialized by Engine's mutex.
unsafe impl Send for Job {}
unsafe impl Sync for Job {}

impl Job {
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
                Error::new(ErrorCode::JobCreate, format!("AssignProcessToJobObject 失败: {e}"))
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
        let mut list = PidList { num_assigned: 0, num_full: 0, pids: [0; 64] };
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

    /// Job 里是否还有活着的进程（detach 后接管时判断服务是否仍在运行）。
    pub fn has_live_process(&self) -> bool {
        !self.pids().is_empty()
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseHandle(self.handle);
        }
    }
}

/// Spawn with CREATE_SUSPENDED, assign to job, then resume.
/// Ceiling: NtResumeProcess is undocumented but is the practical way to resume
/// a std::process::Child created suspended; upgrade is CreateProcess + hThread.
pub fn spawn_in_job(cmd: &mut Command, job: &Job) -> Result<Child> {
    cmd.creation_flags(CREATE_SUSPENDED | CREATE_NO_WINDOW);
    let child = cmd
        .spawn()
        .map_err(|e| Error::new(ErrorCode::Spawn, format!("进程无法启动: {e}")))?;
    job.assign_child(&child)?;
    job.resume_child(&child)?;
    Ok(child)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Stdio;
    use std::time::Duration;

    #[test]
    fn ping_dies_with_job() {
        let job = Job::create().expect("job");
        let mut cmd = Command::new("ping");
        cmd.args(["-t", "127.0.0.1"])
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        let mut child = spawn_in_job(&mut cmd, &job).expect("spawn");
        std::thread::sleep(Duration::from_millis(200));
        assert!(!job.pids().is_empty(), "job 内应有存活 pid");
        job.terminate().expect("term");
        let st = child.wait().expect("wait");
        assert!(!st.success());
    }
}
