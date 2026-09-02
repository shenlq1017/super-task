//! 运行页终端（ipc.md §10.15）：PTY 会话管理。
//!
//! 复用 wezterm 系 `portable-pty`（Windows ConPTY / Unix openpty），不裸写 FFI。
//! 会话是 UI 作用域（跟随前端 Tab 生命周期），不进 Engine 工作区状态机：
//! 壳层持有 `Arc<PtyManager>` 并桥 `st.term` 事件，退出时 `close_all` 清场。
//! 终端不注入 Job Object——它是用户交互进程，关闭 Tab / 退出应用即整树终止
//! （ConPTY 关闭句柄会终止其上挂接的进程树）。

use std::collections::HashMap;
use std::io::{ErrorKind, Read, Write};
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};

use indexmap::IndexMap;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};

use crate::error::{Error, ErrorCode, Result};

/// 同时会话上限（防泄漏；前端每个 Tab 一个会话，8 个足够）。
pub const MAX_SESSIONS: usize = 8;

/// 终端目标：工作目录 + 环境链（由 `Engine::term_target` 组装）。
#[derive(Debug, Clone)]
pub struct TermTarget {
    pub cwd: PathBuf,
    pub env: IndexMap<String, String>,
}

/// 会话事件（经壳层桥到 `st.term`）。data 为 lossy UTF-8（ConPTY 输出 UTF-8）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TermEvent {
    Output { session_id: u64, data: String },
    Exited { session_id: u64, exit_code: i32 },
}

struct PtySession {
    master: Box<dyn portable_pty::MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    killer: Box<dyn portable_pty::ChildKiller + Send + Sync>,
}

#[derive(Default)]
struct PtyInner {
    next_id: u64,
    sessions: HashMap<u64, PtySession>,
}

/// PTY 会话管理器。事件经 mpsc 供壳层桥线程 `try_recv_event` 轮询。
pub struct PtyManager {
    inner: Mutex<PtyInner>,
    tx: Sender<TermEvent>,
    rx: Mutex<Receiver<TermEvent>>,
}

impl PtyManager {
    pub fn new() -> Self {
        let (tx, rx) = channel();
        Self {
            inner: Mutex::new(PtyInner::default()),
            tx,
            rx: Mutex::new(rx),
        }
    }

    /// 打开会话：spawn 终端进程 + 输出泵线程 + 退出等待线程。
    /// 返回 session_id；事件（含 Exited）经 `try_recv_event` 流出。
    pub fn open(self: &Arc<Self>, opts: PtyOpenOptions) -> Result<u64> {
        let id = {
            let mut g = self.inner.lock().expect("pty lock");
            if g.sessions.len() >= MAX_SESSIONS {
                return Err(Error::new(
                    ErrorCode::TermLimit,
                    format!("终端会话已达上限 {MAX_SESSIONS}，请先关闭部分终端"),
                ));
            }
            g.next_id += 1;
            g.next_id
        };
        self.spawn_session(id, opts)?;
        Ok(id)
    }

    fn spawn_session(self: &Arc<Self>, id: u64, opts: PtyOpenOptions) -> Result<()> {
        let cols = opts.cols.clamp(2, 1000);
        let rows = opts.rows.clamp(2, 1000);
        let pair = native_pty_system()
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::new(ErrorCode::TermSpawnFailed, format!("PTY 打开失败: {e}")))?;
        let mut cmd = CommandBuilder::new(&opts.program);
        cmd.args(&opts.args);
        cmd.cwd(&opts.cwd);
        for (k, v) in &opts.env {
            cmd.env(k, v);
        }
        let mut child = pair.slave.spawn_command(cmd).map_err(|e| {
            Error::new(ErrorCode::TermSpawnFailed, format!("终端进程拉起失败: {e}"))
        })?;
        let reader = pair.master.try_clone_reader().map_err(|e| {
            Error::new(
                ErrorCode::TermSpawnFailed,
                format!("PTY 读取端打开失败: {e}"),
            )
        })?;
        let writer = pair.master.take_writer().map_err(|e| {
            Error::new(
                ErrorCode::TermSpawnFailed,
                format!("PTY 写入端打开失败: {e}"),
            )
        })?;
        let killer = child.clone_killer();

        {
            let mut g = self.inner.lock().expect("pty lock");
            g.sessions.insert(
                id,
                PtySession {
                    master: pair.master,
                    writer,
                    killer,
                },
            );
        }

        // 输出泵：EOF / 读错误即结束（ConPTY 随子进程退出关闭）。
        let pump = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("st-term-out-{id}"))
            .spawn(move || {
                let mut reader = reader;
                let mut buf = [0u8; 16 * 1024];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            let data = String::from_utf8_lossy(&buf[..n]).into_owned();
                            let _ = pump.send_event(TermEvent::Output {
                                session_id: id,
                                data,
                            });
                        }
                        Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                        Err(_) => break,
                    }
                }
            })
            .map_err(|e| {
                Error::new(ErrorCode::TermSpawnFailed, format!("输出线程拉起失败: {e}"))
            })?;

        // 退出等待：wait 返回即整会话结束（移除 + Exited 事件）。
        let waiter = Arc::clone(self);
        std::thread::Builder::new()
            .name(format!("st-term-wait-{id}"))
            .spawn(move || {
                let code = match child.wait() {
                    Ok(status) => status.exit_code() as i32,
                    Err(_) => -1,
                };
                waiter.finalize(id, code);
            })
            .map_err(|e| {
                Error::new(ErrorCode::TermSpawnFailed, format!("等待线程拉起失败: {e}"))
            })?;
        Ok(())
    }

    fn send_event(&self, ev: TermEvent) {
        let _ = self.tx.send(ev);
    }

    fn finalize(&self, id: u64, exit_code: i32) {
        self.inner.lock().expect("pty lock").sessions.remove(&id);
        self.send_event(TermEvent::Exited {
            session_id: id,
            exit_code,
        });
    }

    /// 写入用户输入（回车用 `\r`，xterm onData 已是终端序）。
    pub fn write(&self, session_id: u64, data: &str) -> Result<()> {
        let mut g = self.inner.lock().expect("pty lock");
        let Some(s) = g.sessions.get_mut(&session_id) else {
            return Err(Error::new(
                ErrorCode::TermSessionNotFound,
                format!("终端会话不存在或已退出: {session_id}"),
            ));
        };
        s.writer
            .write_all(data.as_bytes())
            .and_then(|_| s.writer.flush())
            .map_err(|e| {
                Error::new(
                    ErrorCode::TermSessionNotFound,
                    format!("终端写入失败（会话可能已退出）: {e}"),
                )
            })
    }

    pub fn resize(&self, session_id: u64, cols: u16, rows: u16) -> Result<()> {
        let g = self.inner.lock().expect("pty lock");
        let Some(s) = g.sessions.get(&session_id) else {
            return Err(Error::new(
                ErrorCode::TermSessionNotFound,
                format!("终端会话不存在或已退出: {session_id}"),
            ));
        };
        s.master
            .resize(PtySize {
                rows: rows.clamp(2, 1000),
                cols: cols.clamp(2, 1000),
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| Error::new(ErrorCode::TermSpawnFailed, format!("终端调整尺寸失败: {e}")))
    }

    /// 关闭会话（幂等：已退出返回 Ok）。kill 触发 EOF + wait → finalize。
    pub fn close(&self, session_id: u64) -> Result<()> {
        let killer = {
            let mut g = self.inner.lock().expect("pty lock");
            g.sessions.remove(&session_id).map(|s| s.killer)
        };
        if let Some(mut killer) = killer {
            let _ = killer.kill();
        }
        Ok(())
    }

    /// 退出/清场：终止全部会话。
    pub fn close_all(&self) {
        let killers: Vec<_> = {
            let mut g = self.inner.lock().expect("pty lock");
            g.sessions.drain().map(|(_, s)| s.killer).collect()
        };
        for mut killer in killers {
            let _ = killer.kill();
        }
    }

    pub fn active_count(&self) -> usize {
        self.inner.lock().expect("pty lock").sessions.len()
    }

    /// 壳层桥线程轮询（模型同 Engine::try_recv_event）。
    pub fn try_recv_event(&self) -> Option<TermEvent> {
        self.rx.lock().expect("pty event lock").try_recv().ok()
    }
}

/// 会话打开参数。`program`/`args` 缺省走 `default_shell()`（UI 永不拼 cmdline——
/// 这里也是后端决定，前端只传 cwd 语义）。
#[derive(Debug, Clone)]
pub struct PtyOpenOptions {
    pub program: String,
    pub args: Vec<String>,
    pub cwd: PathBuf,
    pub env: IndexMap<String, String>,
    pub cols: u16,
    pub rows: u16,
}

impl PtyOpenOptions {
    /// 用平台默认 shell 打开（PowerShell 优先，回落 cmd / $SHELL）。
    pub fn with_default_shell(
        cwd: PathBuf,
        env: IndexMap<String, String>,
        cols: u16,
        rows: u16,
    ) -> Self {
        let (program, args) = default_shell();
        Self {
            program,
            args,
            cwd,
            env,
            cols,
            rows,
        }
    }
}

/// 平台默认 shell（Windows 优先 PowerShell——支持 ANSI 与 UTF-8 输出，回落 cmd）。
pub fn default_shell() -> (String, Vec<String>) {
    #[cfg(windows)]
    {
        if let Some(root) = std::env::var_os("SystemRoot") {
            let ps = PathBuf::from(root).join("System32\\WindowsPowerShell\\v1.0\\powershell.exe");
            if ps.is_file() {
                return (ps.to_string_lossy().into_owned(), vec!["-NoLogo".into()]);
            }
        }
        let comspec = std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".into());
        (comspec, vec![])
    }
    #[cfg(not(windows))]
    {
        let shell = std::env::var("SHELL").unwrap_or_default();
        if !shell.is_empty() {
            return (shell, vec![]);
        }
        for candidate in ["/bin/bash", "/bin/sh"] {
            if PathBuf::from(candidate).is_file() {
                return (candidate.into(), vec![]);
            }
        }
        ("/bin/sh".into(), vec![])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_shell_returns_existing_program() {
        let (program, _args) = default_shell();
        assert!(!program.is_empty());
        // Windows 下 PowerShell 或 cmd 必然其一存在；Unix 下 SHELL/常见路径存在
        assert!(PathBuf::from(&program).is_file() || program.ends_with("cmd.exe"));
    }

    #[test]
    fn missing_session_write_resize_errors_and_close_idempotent() {
        let mgr = PtyManager::new();
        let err = mgr.write(999, "hi").unwrap_err();
        assert_eq!(err.code(), ErrorCode::TermSessionNotFound);
        let err = mgr.resize(999, 80, 24).unwrap_err();
        assert_eq!(err.code(), ErrorCode::TermSessionNotFound);
        // close 幂等：不存在也 Ok
        mgr.close(999).unwrap();
        mgr.close_all();
        assert_eq!(mgr.active_count(), 0);
        assert!(mgr.try_recv_event().is_none());
    }

    /// 真机 ConPTY 冒烟：opt-in 手工跑（CI 无头环境不默认执行）。
    /// 走真实交互路径：开 shell → 代答 ConPTY 启动 DSR 握手（真实前端由
    /// xterm.js 自动回应）→ 写入 echo → 校验回显 → exit → 校验退出清场。
    /// `cargo test -p supertask-core term:: -- --ignored`
    #[test]
    #[ignore = "真机 PTY 冒烟：需要本机 ConPTY/openpty 支持"]
    fn real_pty_echo_smoke() {
        let mgr = Arc::new(PtyManager::new());
        let cwd = std::env::temp_dir();
        let opts = if cfg!(windows) {
            PtyOpenOptions {
                program: "cmd.exe".into(),
                args: vec!["/k".into()],
                cwd,
                env: IndexMap::new(),
                cols: 80,
                rows: 24,
            }
        } else {
            PtyOpenOptions {
                program: "/bin/sh".into(),
                args: vec![],
                cwd,
                env: IndexMap::new(),
                cols: 80,
                rows: 24,
            }
        };
        let id = mgr.open(opts).expect("open pty");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        let mut output = String::new();
        let pump = |mgr: &PtyManager, output: &mut String| {
            while let Some(ev) = mgr.try_recv_event() {
                if let TermEvent::Output { data, .. } = ev {
                    output.push_str(&data);
                }
            }
        };
        // ConPTY 启动握手：conhost 发 \x1b[6n 等终端回 DSR 光标报告后才渲染
        mgr.write(id, "\x1b[1;1R").expect("write dsr");
        std::thread::sleep(std::time::Duration::from_millis(300));
        pump(&mgr, &mut output);
        mgr.write(id, "echo st-term-ok\r").expect("write cmd");
        while std::time::Instant::now() < deadline && !output.contains("st-term-ok") {
            pump(&mgr, &mut output);
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(
            output.contains("st-term-ok"),
            "echo output missing; output was: {output:?}"
        );
        mgr.write(id, "exit\r").expect("write exit");
        let mut exited = false;
        while std::time::Instant::now() < deadline {
            match mgr.try_recv_event() {
                Some(TermEvent::Exited { .. }) => {
                    exited = true;
                    break;
                }
                Some(TermEvent::Output { data, .. }) => output.push_str(&data),
                None => std::thread::sleep(std::time::Duration::from_millis(20)),
            }
        }
        assert!(exited, "smoke session did not exit in time");
        assert_eq!(mgr.active_count(), 0, "session should be finalized");
    }
}
