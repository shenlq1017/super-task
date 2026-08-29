//! 进程树平台层（1.4 §3）：同一 trait 契约，Windows Job Object / Unix 进程组两套实现。
//!
//! 守恒规则：业务代码（状态机、拓扑、日志）不出现 `if os`；
//! 平台差异只存在于本模块的 cfg 分支内。
//! Windows 行为零变化：`windows.rs` 是原 `job.rs` 的整体迁入，错误码与行为不变。

#[cfg(unix)]
pub mod unix;
#[cfg(windows)]
pub mod windows;

use std::process::{Child, Command};

use crate::error::Result;

/// 一个服务（或脚本）的整棵进程树句柄。
///
/// 契约：`spawn` 后子进程及全部后代归本句柄管辖；
/// `terminate` 覆盖整棵树；`pids`/指标查询失败时降级（空列表 / None），不误判服务退出。
pub trait ProcessTree: Send + Sync {
    /// 启动子进程并纳入管辖（Windows: CREATE_SUSPENDED + assign + resume；
    /// Unix: 独立进程组，Linux 额外 PDEATHSIG）。
    fn spawn(&self, cmd: &mut Command) -> Result<Child>;

    /// 终止整棵进程树。Windows: TerminateJobObject；Unix: SIGTERM → 5s 宽限 → SIGKILL。
    fn terminate(&self) -> Result<()>;

    /// 当前存活的 pid 列表；查询失败返回空列表。
    fn pids(&self) -> Vec<u32>;

    /// 进程树里是否还有活着的进程（detach 后接管时判断服务是否仍在运行）。
    fn has_live_process(&self) -> bool {
        !self.pids().is_empty()
    }

    /// 累计 CPU 时间（内核+用户，毫秒）；查询失败返回 None（不判服务异常）。
    fn total_cpu_ms(&self) -> Option<u64>;

    /// 进程树内存占用（字节）；部分可用返回部分和，全部失败 None。
    fn working_set_bytes(&self) -> Option<u64>;
}

/// 创建一棵进程树句柄（按编译平台分发）。
#[cfg(windows)]
pub fn create_tree() -> Result<std::sync::Arc<dyn ProcessTree>> {
    Ok(std::sync::Arc::new(windows::WindowsJob::create()?))
}

#[cfg(unix)]
pub fn create_tree() -> Result<std::sync::Arc<dyn ProcessTree>> {
    Ok(std::sync::Arc::new(unix::UnixProcessGroup::new()))
}

/// 任意 pid 是否存活（1.5 工作区锁 stale 判定；按编译平台分发，只读探测）。
#[cfg(windows)]
pub fn pid_alive(pid: u32) -> bool {
    windows::pid_alive(pid)
}

#[cfg(unix)]
pub fn pid_alive(pid: u32) -> bool {
    unix::pid_alive(pid)
}
