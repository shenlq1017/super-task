//! 运行页终端（ipc.md §10.15）：PTY 会话的壳层适配。
//! 业务在 `supertask-core::term`（portable-pty）；这里只做命令转发与 `st.term` 事件桥。

use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use supertask_core::error::ErrorCode;
use supertask_core::ipc::{IpcError, TermEventPayload, TermOpenOutput, PROTOCOL};
use supertask_core::term::{PtyManager, PtyOpenOptions};

use crate::commands::{ensure_not_exiting, err, ipc_err, Accepted, EngineState};

/// 托管句柄（lib.rs manage；桥线程与退出清场共用同一 Arc）。
pub struct TermHandle(pub Arc<PtyManager>);

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[derive(Debug, Serialize)]
struct TermEventEnvelope {
    protocol: u32,
    event: &'static str,
    workspace_id: Option<String>,
    ts_ms: u64,
    payload: TermEventPayload,
}

/// `st.term` 桥线程（模型同 spawn_event_bridge）：轮询 PtyManager 事件并 emit。
pub fn spawn_term_bridge(app: AppHandle, term: Arc<PtyManager>) {
    std::thread::Builder::new()
        .name("st-term-bridge".into())
        .spawn(move || loop {
            match term.try_recv_event() {
                Some(ev) => {
                    let payload = match ev {
                        supertask_core::term::TermEvent::Output { session_id, data } => {
                            TermEventPayload::output(session_id, data)
                        }
                        supertask_core::term::TermEvent::Exited {
                            session_id,
                            exit_code,
                        } => TermEventPayload::exited(session_id, exit_code),
                    };
                    let envelope = TermEventEnvelope {
                        protocol: PROTOCOL,
                        event: supertask_core::ipc::event::TERM,
                        workspace_id: None,
                        ts_ms: now_ms(),
                        payload,
                    };
                    let _ = app.emit(supertask_core::ipc::event::TERM, &envelope);
                }
                None => std::thread::sleep(Duration::from_millis(10)),
            }
        })
        .expect("spawn term bridge");
}

/// 打开终端会话：cwd/环境链由引擎给出（服务终端 = 服务 cwd + §6.3 环境链 +
/// 1.7 §7 镜像注入），程序为平台默认 shell（后端决定，UI 永不拼 cmdline）。
#[tauri::command(rename = "term.open")]
pub fn term_open(
    engine: EngineState<'_>,
    term: State<'_, TermHandle>,
    exiting: State<'_, crate::state::Exiting>,
    workspace_id: String,
    service_id: Option<String>,
    cols: Option<u16>,
    rows: Option<u16>,
) -> Result<TermOpenOutput, IpcError> {
    ensure_not_exiting(&exiting)?;
    let current = engine.workspace_id().map_err(ipc_err)?;
    if workspace_id != current {
        return Err(err(
            ErrorCode::NoWorkspace,
            format!("workspace_id 不匹配当前工作区: {workspace_id}"),
        ));
    }
    let target = engine.term_target(service_id.as_deref()).map_err(ipc_err)?;
    let opts = PtyOpenOptions::with_default_shell(
        target.cwd,
        target.env,
        cols.unwrap_or(80),
        rows.unwrap_or(24),
    );
    let shell = opts.program.clone();
    let session_id = term.0.open(opts).map_err(ipc_err)?;
    Ok(TermOpenOutput { session_id, shell })
}

#[tauri::command(rename = "term.write")]
pub fn term_write(
    term: State<'_, TermHandle>,
    session_id: u64,
    data: String,
) -> Result<Accepted, IpcError> {
    term.0.write(session_id, &data).map_err(ipc_err)?;
    Ok(Accepted { accepted: true, order: None })
}

#[tauri::command(rename = "term.resize")]
pub fn term_resize(
    term: State<'_, TermHandle>,
    session_id: u64,
    cols: u16,
    rows: u16,
) -> Result<Accepted, IpcError> {
    term.0.resize(session_id, cols, rows).map_err(ipc_err)?;
    Ok(Accepted { accepted: true, order: None })
}

/// 关闭会话（幂等）。ConPTY 句柄关闭会终止其上进程树，无需 Job Object。
#[tauri::command(rename = "term.close")]
pub fn term_close(
    term: State<'_, TermHandle>,
    session_id: u64,
) -> Result<Accepted, IpcError> {
    term.0.close(session_id).map_err(ipc_err)?;
    Ok(Accepted { accepted: true, order: None })
}
