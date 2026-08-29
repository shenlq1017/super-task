//! 1.1 长操作 hub：clone / pull / 模板创建 / 更新统一走 operation。
//! Spec: `docs/spec/ipc.md` §10.0。
//!
//! 语义（硬性）：
//! - 事件序列 `Queued → (Running+report…)* → 终态`；**终态唯一**，进入
//!   Succeeded/Failed 后一切后续写入（含闭包内部迟到 report）被忽略；
//! - `spawn` 立即入队（Command 层目标 < 50ms），后台线程执行；
//! - 事件按发生顺序经 unbounded channel 排队，由事件桥 `try_recv_event` 轮询。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use serde::Serialize;

use crate::error::{Error, ErrorCode, Result};

/// 操作状态机。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpState {
    Queued,
    Running,
    Succeeded,
    Failed,
    /// 1.3：用户取消（best effort，已提交的层不回滚，规格 §3.2/§6.2）。
    Cancelled,
}

/// `st.operation` 事件负载（ts 由桥层补充，这里不带）。
#[derive(Debug, Clone, Serialize)]
pub struct OperationEvent {
    pub operation_id: String,
    pub kind: String,
    pub state: OpState,
    pub progress: Option<f64>,
    pub message: Option<String>,
    pub error_code: Option<String>,
    pub result: Option<serde_yaml::Value>,
}

/// 单个操作的最新记录；`terminal` 后一切写入被忽略（终态唯一）。
struct OperationRecord {
    kind: String,
    state: OpState,
    progress: Option<f64>,
    message: Option<String>,
    error_code: Option<String>,
    result: Option<serde_yaml::Value>,
    terminal: bool,
    /// 1.3：取消标记 + 终止句柄（构建杀进程等）。
    cancel: Arc<AtomicBool>,
    kill: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}

/// hub 共享内核：记录表 + 事件发送端（后台线程经 Arc 持有）。
struct HubInner {
    records: Mutex<HashMap<String, OperationRecord>>,
    tx: Sender<OperationEvent>,
}

impl HubInner {
    /// 统一状态写入 + 按序入队事件。进入终态后忽略一切后续写入。
    /// `progress/message` 传 `None` 表示"保持原值"。
    fn transition(
        &self,
        id: &str,
        state: OpState,
        progress: Option<f64>,
        message: Option<String>,
        error_code: Option<String>,
        result: Option<serde_yaml::Value>,
    ) {
        let event = {
            let mut records = self.records.lock().unwrap();
            let record = match records.get_mut(id) {
                Some(r) => r,
                None => return,
            };
            if record.terminal {
                return; // 终态唯一
            }
            record.state = state;
            if let Some(p) = progress {
                record.progress = Some(p);
            }
            if let Some(m) = message {
                record.message = Some(m);
            }
            record.error_code = error_code;
            record.result = result;
            if matches!(
                state,
                OpState::Succeeded | OpState::Failed | OpState::Cancelled
            ) {
                record.terminal = true;
            }
            OperationEvent {
                operation_id: id.to_string(),
                kind: record.kind.clone(),
                state: record.state,
                progress: record.progress,
                message: record.message.clone(),
                error_code: record.error_code.clone(),
                result: record.result.clone(),
            }
        };
        // 接收端与 hub 同生命周期；发送失败只意味着无人订阅，丢弃即可
        let _ = self.tx.send(event);
    }
}

/// 长操作 hub：立即入队 + 后台线程执行 + 按序事件流。
pub struct OperationHub {
    inner: Arc<HubInner>,
    rx: Mutex<Receiver<OperationEvent>>,
    counter: AtomicU64,
}

impl Default for OperationHub {
    fn default() -> Self {
        Self::new()
    }
}

impl OperationHub {
    pub fn new() -> Self {
        // std::sync::mpsc::channel 即无界通道（unbounded_channel 是 crossbeam 命名）
        let (tx, rx) = channel();
        Self {
            inner: Arc::new(HubInner {
                records: Mutex::new(HashMap::new()),
                tx,
            }),
            rx: Mutex::new(rx),
            counter: AtomicU64::new(0),
        }
    }

    /// 立即入队（同步写入 Queued 记录 + 发 Queued 事件）并在后台线程执行。
    /// 闭包成功 → `Succeeded(result)`；Err → `Failed(code, message)`；panic 也落到 Failed。
    /// 返回 operation_id，Command 层应立即把它返回给前端。
    pub fn spawn<F>(&self, kind: &str, f: F) -> String
    where
        F: FnOnce(&OperationCtx) -> Result<serde_yaml::Value> + Send + 'static,
    {
        let n = self.counter.fetch_add(1, Ordering::Relaxed) + 1;
        let id = format!("op-{n}");
        let kind = kind.to_string();

        self.inner.records.lock().unwrap().insert(
            id.clone(),
            OperationRecord {
                kind: kind.clone(),
                state: OpState::Queued,
                progress: None,
                message: None,
                error_code: None,
                result: None,
                terminal: false,
                cancel: Arc::new(AtomicBool::new(false)),
                kill: Mutex::new(None),
            },
        );
        self.inner
            .transition(&id, OpState::Queued, None, None, None, None);

        let ctx = OperationCtx {
            id: id.clone(),
            inner: Arc::clone(&self.inner),
            cancel: {
                let records = self.inner.records.lock().unwrap();
                records
                    .get(&id)
                    .map(|r| r.cancel.clone())
                    .unwrap_or_default()
            },
        };
        let inner = Arc::clone(&self.inner);
        let id_for_thread = id.clone();
        std::thread::spawn(move || {
            let id = id_for_thread;
            inner.transition(&id, OpState::Running, None, None, None, None);
            // panic 也必须落到终态，否则记录永久卡在 Running（has_active 永真）
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| f(&ctx)))
                .unwrap_or_else(|_| {
                    Err(Error::new(
                        ErrorCode::Protocol,
                        "operation 内部错误（panic）",
                    ))
                });
            match outcome {
                Ok(value) => {
                    inner.transition(&id, OpState::Succeeded, None, None, None, Some(value));
                }
                Err(err) => inner.transition(
                    &id,
                    OpState::Failed,
                    None,
                    Some(err.to_string()),
                    Some(error_code_string(err.code())),
                    None,
                ),
            }
        });
        id
    }

    /// 供事件桥轮询：按发生顺序取一条事件；无事件时 `None`。
    pub fn try_recv_event(&self) -> Option<OperationEvent> {
        self.rx.lock().unwrap().try_recv().ok()
    }

    /// 最新状态快照（不消耗事件流）。
    pub fn get(&self, id: &str) -> Option<OperationEvent> {
        let records = self.inner.records.lock().unwrap();
        records.get(id).map(|r| OperationEvent {
            operation_id: id.to_string(),
            kind: r.kind.clone(),
            state: r.state,
            progress: r.progress,
            message: r.message.clone(),
            error_code: r.error_code.clone(),
            result: r.result.clone(),
        })
    }

    /// 是否存在 queued/running 操作（更新等前置检查用）。
    pub fn has_active(&self) -> bool {
        let records = self.inner.records.lock().unwrap();
        records
            .values()
            .any(|r| matches!(r.state, OpState::Queued | OpState::Running))
    }

    /// 1.3：请求取消。best effort：置位取消标记并执行注册的 kill（杀构建进程），
    /// 状态转 `cancelled`（终态唯一）。已终态 → false。
    pub fn cancel(&self, id: &str) -> bool {
        let (cancel, kill) = {
            let records = self.inner.records.lock().unwrap();
            let Some(r) = records.get(id) else {
                return false;
            };
            if r.terminal {
                return false;
            }
            let kill = r.kill.lock().unwrap().take();
            (r.cancel.clone(), kill)
        };
        cancel.store(true, Ordering::SeqCst);
        if let Some(kill) = kill {
            kill();
        }
        self.inner
            .transition(id, OpState::Cancelled, None, None, None, None);
        true
    }
}

/// 错误码序列化：`ErrorCode` 已按 SCREAMING_SNAKE_CASE 派生 Serialize。
fn error_code_string(code: ErrorCode) -> String {
    serde_yaml::to_string(&code)
        .unwrap_or_else(|_| "PROTOCOL".to_string())
        .trim()
        .to_string()
}

/// 传给 operation 闭包的上下文：只暴露进度上报与取消检查，不暴露 hub 内部。
#[derive(Clone)]
pub struct OperationCtx {
    id: String,
    inner: Arc<HubInner>,
    cancel: Arc<AtomicBool>,
}

impl OperationCtx {
    /// 运行中进度上报（progress 可为 None，不伪造无法测量的百分比）。
    /// 终态后调用无效。
    pub fn report(&self, progress: Option<f64>, message: impl Into<String>) {
        self.inner.transition(
            &self.id,
            OpState::Running,
            progress,
            Some(message.into()),
            None,
            None,
        );
    }

    /// 1.3：是否已被请求取消（长操作循环里按行/按块检查）。
    pub fn cancelled(&self) -> bool {
        self.cancel.load(Ordering::SeqCst)
    }

    /// 1.3：注册取消时的终止动作（杀构建进程等），只保留最后一次注册。
    pub fn on_cancel(&self, kill: Box<dyn FnOnce() + Send>) {
        let records = self.inner.records.lock().unwrap();
        if let Some(r) = records.get(&self.id) {
            *r.kill.lock().unwrap() = Some(kill);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;
    use std::time::{Duration, Instant};

    /// 轮询事件流直到收集到 n 个终态事件或超时（5s）。
    fn wait_terminals(hub: &OperationHub, n: usize) -> Vec<OperationEvent> {
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut terminals = Vec::new();
        while terminals.len() < n && Instant::now() < deadline {
            match hub.try_recv_event() {
                Some(event) => {
                    if matches!(event.state, OpState::Succeeded | OpState::Failed) {
                        terminals.push(event);
                    }
                }
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        assert_eq!(terminals.len(), n, "等待终态事件超时");
        terminals
    }

    #[test]
    fn lifecycle_event_order() {
        let hub = OperationHub::new();
        let id = hub.spawn("test.kind", |ctx| {
            ctx.report(Some(0.3), "步骤 1");
            ctx.report(Some(0.6), "步骤 2");
            Ok(serde_yaml::Value::String("done".into()))
        });
        assert!(id.starts_with("op-"));

        // 依次收集到终态为止
        let deadline = Instant::now() + Duration::from_secs(5);
        let mut events = Vec::new();
        while Instant::now() < deadline {
            match hub.try_recv_event() {
                Some(e) => {
                    let terminal = matches!(e.state, OpState::Succeeded | OpState::Failed);
                    events.push(e);
                    if terminal {
                        break;
                    }
                }
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        }
        let states: Vec<OpState> = events.iter().map(|e| e.state).collect();
        assert_eq!(
            states,
            vec![
                OpState::Queued,
                OpState::Running,
                OpState::Running,
                OpState::Running,
                OpState::Succeeded,
            ],
            "事件序列应为 Queued → Running(启动) → Running(report…)* → 终态"
        );
        assert_eq!(events[0].operation_id, id);
        assert_eq!(events[0].kind, "test.kind");
        assert_eq!(events[2].message.as_deref(), Some("步骤 1"));
        assert_eq!(events[2].progress, Some(0.3));
        assert_eq!(events[3].message.as_deref(), Some("步骤 2"));
        assert_eq!(
            events[4].result,
            Some(serde_yaml::Value::String("done".into()))
        );
        assert!(events[4].error_code.is_none());

        // 快照与终态一致，且不再活跃
        let snap = hub.get(&id).unwrap();
        assert_eq!(snap.state, OpState::Succeeded);
        assert!(!hub.has_active());
    }

    #[test]
    fn failure_carries_error_code() {
        let hub = OperationHub::new();
        let id = hub.spawn("test.fail", |_| {
            Err(Error::new(ErrorCode::IdeNotFound, "未找到 IDE"))
        });
        let terminals = wait_terminals(&hub, 1);
        let failed = &terminals[0];
        assert_eq!(failed.operation_id, id);
        assert_eq!(failed.state, OpState::Failed);
        assert_eq!(failed.error_code.as_deref(), Some("IDE_NOT_FOUND"));
        assert!(failed.message.as_deref().unwrap().contains("未找到 IDE"));
        assert!(failed.result.is_none());
    }

    #[test]
    fn report_after_terminal_is_ignored() {
        let hub = OperationHub::new();
        // f 返回后，由迟到的线程再 report —— 必须被忽略且不产生事件
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let id = hub.spawn("test.late", move |ctx| {
            let late_ctx = ctx.clone();
            std::thread::spawn(move || {
                let _ = rx.recv(); // 等主测试确认终态后再上报
                late_ctx.report(Some(0.9), "迟到的上报");
            });
            Ok(serde_yaml::Value::Null)
        });
        wait_terminals(&hub, 1);
        tx.send(()).unwrap();
        std::thread::sleep(Duration::from_millis(150));
        assert!(hub.try_recv_event().is_none(), "终态后不允许再产生事件");
        let snap = hub.get(&id).unwrap();
        assert_eq!(snap.state, OpState::Succeeded);
        assert_ne!(snap.progress, Some(0.9));
    }

    #[test]
    fn has_active_tracks_lifecycle() {
        let hub = OperationHub::new();
        assert!(!hub.has_active());
        let id = hub.spawn("test.active", |_| {
            std::thread::sleep(Duration::from_millis(120));
            Ok(serde_yaml::Value::Null)
        });
        // spawn 同步插入 Queued 记录，返回后必然活跃
        assert!(hub.has_active());
        wait_terminals(&hub, 1);
        assert_eq!(hub.get(&id).unwrap().state, OpState::Succeeded);
        assert!(!hub.has_active());
    }

    #[test]
    fn cancel_transitions_to_cancelled_and_fires_kill() {
        use std::sync::atomic::AtomicUsize;
        let hub = OperationHub::new();
        let killed = Arc::new(AtomicUsize::new(0));
        let killed2 = Arc::clone(&killed);
        let (registered_tx, registered_rx) = std::sync::mpsc::channel::<()>();
        let id = hub.spawn("test.cancel", move |ctx| {
            ctx.on_cancel(Box::new(move || {
                killed2.fetch_add(1, Ordering::SeqCst);
            }));
            let _ = registered_tx.send(());
            // 模拟长构建：等取消或超时
            for _ in 0..200 {
                if ctx.cancelled() {
                    return Err(Error::new(ErrorCode::Spawn, "已取消"));
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Ok(serde_yaml::Value::Null)
        });
        assert!(hub.has_active());
        // 等 on_cancel 注册完成再取消，避免注册与取消竞态
        registered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("on_cancel should register");
        assert!(hub.cancel(&id));
        let snap = hub.get(&id).unwrap();
        assert_eq!(snap.state, OpState::Cancelled);
        assert_eq!(killed.load(Ordering::SeqCst), 1, "kill 句柄应被触发");
        // 终态后重复取消无效
        assert!(!hub.cancel(&id));
        // 迟到的 Failed/Ok 被忽略，终态唯一
        std::thread::sleep(Duration::from_millis(120));
        assert_eq!(hub.get(&id).unwrap().state, OpState::Cancelled);
        assert!(!hub.has_active());
        // 未知 id → false
        assert!(!hub.cancel("op-999"));
    }

    #[test]
    fn concurrent_spawn_unique_terminal() {
        let hub = OperationHub::new();
        for i in 0..10 {
            hub.spawn("test.concurrent", move |ctx| {
                ctx.report(None, format!("任务 {i}"));
                if i % 2 == 0 {
                    Ok(serde_yaml::Value::Number(i.into()))
                } else {
                    Err(Error::new(ErrorCode::Discover, format!("任务 {i} 失败")))
                }
            });
        }
        let terminals = wait_terminals(&hub, 10);
        let ids: HashSet<&str> = terminals.iter().map(|e| e.operation_id.as_str()).collect();
        assert_eq!(ids.len(), 10, "每个 operation 只能有一个终态事件");
        for event in &terminals {
            let snap = hub.get(&event.operation_id).unwrap();
            assert_eq!(snap.state, event.state);
            assert!(matches!(snap.state, OpState::Succeeded | OpState::Failed));
            match snap.state {
                OpState::Succeeded => assert!(snap.result.is_some()),
                OpState::Failed => assert!(snap.error_code.is_some()),
                _ => unreachable!(),
            }
        }
        assert!(!hub.has_active());
    }
}
