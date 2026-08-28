use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RtState {
    Stopped,
    /// 1.2 §11：launch: jar 的 package 阶段；进入 jar 进程后转 Starting。
    Building,
    Starting,
    Running,
    Unhealthy,
    Stopping,
    Exited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RtEvent {
    Spawned { health_none: bool },
    SpawnFailed,
    HealthOk,
    HealthFail { past_grace: bool },
    ProcessExited { stop_requested: bool },
    StopRequested,
    StopCompleted,
    /// 1.2：package 开始（Stopped/Exited → Building）。
    BuildStarted,
    /// 1.2：package 结束；ok=false 时由调用方写 last_error（BUILD_FAILED）。
    BuildFinished { ok: bool },
}

pub fn apply(state: RtState, event: RtEvent) -> Result<RtState> {
    use RtEvent::*;
    use RtState::*;
    let next = match (state, event) {
        (Stopped | Exited, BuildStarted) => Building,
        (Building, BuildFinished { .. }) => Stopped,
        (Building, StopRequested) => Stopping,
        (Building, ProcessExited { stop_requested: true }) => Stopped,
        (Stopped | Exited, Spawned { health_none: true }) => Running,
        (Stopped | Exited, Spawned { health_none: false }) => Starting,
        (Stopped | Exited, SpawnFailed) => Exited,
        (Starting, HealthOk) => Running,
        (Starting, HealthFail { past_grace: false }) => Starting,
        (Starting, HealthFail { past_grace: true }) => Unhealthy,
        (Starting, ProcessExited { stop_requested: true }) => Stopped,
        (Starting, ProcessExited { stop_requested: false }) => Exited,
        (Starting, StopRequested) => Stopping,
        (Running, HealthOk) => Running,
        (Running, HealthFail { past_grace: true }) => Unhealthy,
        (Running, HealthFail { past_grace: false }) => Running,
        (Running, ProcessExited { stop_requested: true }) => Stopped,
        (Running, ProcessExited { stop_requested: false }) => Exited,
        (Running, StopRequested) => Stopping,
        (Unhealthy, HealthOk) => Running,
        (Unhealthy, HealthFail { .. }) => Unhealthy,
        (Unhealthy, ProcessExited { stop_requested: true }) => Stopped,
        (Unhealthy, ProcessExited { stop_requested: false }) => Exited,
        (Unhealthy, StopRequested) => Stopping,
        (Stopping, StopCompleted | ProcessExited { stop_requested: true }) => Stopped,
        (Stopping, ProcessExited { stop_requested: false }) => Stopped,
        (Stopping, StopRequested) => Stopping,
        (Exited, StopRequested) => Stopped,
        (Stopped, StopRequested | StopCompleted) => Stopped,
        (other, ev) => {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("非法状态变迁 {other:?} + {ev:?}"),
            ));
        }
    };
    Ok(next)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn happy_path() {
        let mut s = RtState::Stopped;
        s = apply(s, RtEvent::Spawned { health_none: false }).unwrap();
        assert_eq!(s, RtState::Starting);
        s = apply(s, RtEvent::HealthFail { past_grace: false }).unwrap();
        assert_eq!(s, RtState::Starting);
        s = apply(s, RtEvent::HealthOk).unwrap();
        assert_eq!(s, RtState::Running);
        s = apply(s, RtEvent::StopRequested).unwrap();
        s = apply(s, RtEvent::StopCompleted).unwrap();
        assert_eq!(s, RtState::Stopped);
    }

    #[test]
    fn crash_to_exited() {
        let mut s = apply(RtState::Running, RtEvent::ProcessExited { stop_requested: false }).unwrap();
        assert_eq!(s, RtState::Exited);
        s = apply(s, RtEvent::Spawned { health_none: true }).unwrap();
        assert_eq!(s, RtState::Running);
    }

    #[test]
    fn no_health_skips_starting() {
        let s = apply(RtState::Stopped, RtEvent::Spawned { health_none: true }).unwrap();
        assert_eq!(s, RtState::Running);
    }

    #[test]
    fn build_phase_transitions() {
        // 构建 → 成功回 Stopped（随后 Spawned 进 Starting）
        let s = apply(RtState::Stopped, RtEvent::BuildStarted).unwrap();
        assert_eq!(s, RtState::Building);
        let s = apply(s, RtEvent::BuildFinished { ok: true }).unwrap();
        assert_eq!(s, RtState::Stopped);
        let s = apply(s, RtEvent::Spawned { health_none: false }).unwrap();
        assert_eq!(s, RtState::Starting);
        // 构建失败同样回到 Stopped，错误由调用方写 last_error
        let s = apply(RtState::Exited, RtEvent::BuildStarted).unwrap();
        let s = apply(s, RtEvent::BuildFinished { ok: false }).unwrap();
        assert_eq!(s, RtState::Stopped);
        // 构建中被停止
        let s = apply(RtState::Building, RtEvent::ProcessExited { stop_requested: true }).unwrap();
        assert_eq!(s, RtState::Stopped);
        // Building 期间视为忙：不允许再 Spawned
        assert!(apply(RtState::Building, RtEvent::Spawned { health_none: true }).is_err());
    }
}
