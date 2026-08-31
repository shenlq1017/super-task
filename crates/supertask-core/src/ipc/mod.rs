use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode};

pub const MAX_YAML_BYTES: usize = 1024 * 1024;
pub const MAX_SERVICES: usize = 64;
pub const MAX_CMDS: usize = 32;
pub const MAX_ENV_KEYS: usize = 256;
pub const MAX_LOG_LINE_BYTES: usize = 8 * 1024;
pub const DEFAULT_RING_LINES: usize = 2000;
pub const DEFAULT_LOG_MAX_BYTES: u64 = 10 * 1024 * 1024;
pub const LOG_BATCH_ITEMS: usize = 32;
pub const LOG_BATCH_MS: u64 = 50;
pub const PROTOCOL: u32 = 1;

mod v12;
pub use v12::*;

mod v13;
pub use v13::*;

mod v15;
pub use v15::*;

mod v16;
pub use v16::*;

/// Stable invoke names. Tauri layer must register these (or map 1:1).
pub mod cmd {
    pub const SESSION_HELLO: &str = "session.hello";
    pub const APP_LOAD: &str = "app.load";
    pub const APP_SAVE_PREFS: &str = "app.savePrefs";
    pub const WORKSPACE_ADD: &str = "workspace.add";
    pub const WORKSPACE_OPEN: &str = "workspace.open";
    pub const WORKSPACE_CLOSE: &str = "workspace.close";
    pub const WORKSPACE_FORGET: &str = "workspace.forget";
    pub const WORKSPACE_SCAN_DRAFT: &str = "workspace.scanDraft";
    pub const WORKSPACE_OPEN_EXPLORER: &str = "workspace.openExplorer";
    pub const YAML_GET: &str = "yaml.get";
    pub const YAML_SAVE_TEXT: &str = "yaml.saveText";
    pub const YAML_SAVE_FORM: &str = "yaml.saveForm";
    pub const RUNTIME_SNAPSHOT: &str = "runtime.snapshot";
    pub const RUNTIME_START_ONE: &str = "runtime.startOne";
    pub const RUNTIME_START_ALL: &str = "runtime.startAll";
    pub const RUNTIME_STOP_ONE: &str = "runtime.stopOne";
    pub const RUNTIME_STOP_ALL: &str = "runtime.stopAll";
    pub const RUNTIME_RESTART_ONE: &str = "runtime.restartOne";
    pub const SCRIPT_RUN: &str = "script.run";
    pub const SCRIPT_CANCEL: &str = "script.cancel";
    pub const TOOLCHAIN_PROBE: &str = "toolchain.probe";
    pub const LOGS_SUBSCRIBE: &str = "logs.subscribe";
    pub const LOGS_UNSUBSCRIBE: &str = "logs.unsubscribe";
    pub const LOGS_SNAPSHOT: &str = "logs.snapshot";
    pub const LOGS_CLEAR_VIEW: &str = "logs.clearView";
    // ---- 1.2 (types only; handlers in later phases) ----
    pub const TOOLCHAIN_INSTALL: &str = "toolchain.install";
    pub const TOOLCHAIN_UPGRADE: &str = "toolchain.upgrade";
    pub const PORTS_INSPECT: &str = "ports.inspect";
    pub const PORTS_SUGGEST: &str = "ports.suggest";
    pub const PORTS_ASSIGN: &str = "ports.assign";
    pub const SECRETS_STATUS: &str = "secrets.status";
    pub const SECRETS_SET: &str = "secrets.set";
    pub const SECRETS_DELETE: &str = "secrets.delete";
    pub const SECRETS_VALIDATE: &str = "secrets.validate";
    pub const NETWORK_SAVE: &str = "network.save";
    pub const LOGS_SEARCH: &str = "logs.search";
    pub const LOGS_EXPORT: &str = "logs.export";
    pub const LOGS_RETENTION_RUN: &str = "logs.retention.run";
    pub const METRICS_SNAPSHOT: &str = "metrics.snapshot";
    pub const METRICS_SUBSCRIBE: &str = "metrics.subscribe";
    pub const METRICS_UNSUBSCRIBE: &str = "metrics.unsubscribe";
    pub const PROFILES_LIST: &str = "profiles.list";
    pub const PROFILES_ACTIVATE: &str = "profiles.activate";
    pub const RUNTIME_BUILD: &str = "runtime.build";
    // ---- 1.3 (types only; handlers in later phases) ----
    pub const DOCKER_PROBE: &str = "docker.probe";
    pub const DOCKER_PS: &str = "docker.ps";
    pub const DOCKER_IMAGES: &str = "docker.images";
    pub const DOCKER_BUILD: &str = "docker.build";
    // ---- 1.4 (types only; handlers in later phases) ----
    pub const IMPORT_TASKFILE_PREVIEW: &str = "import.taskfilePreview";
    pub const IMPORT_TASKFILE_APPLY: &str = "import.taskfileApply";
    // ---- 1.6 ----
    pub const GATEWAY_STATUS: &str = "gateway.status";
    pub const GATEWAY_PREVIEW: &str = "gateway.preview";
    pub const GATEWAY_VALIDATE: &str = "gateway.validate";
    pub const GATEWAY_APPLY: &str = "gateway.apply";
    pub const GATEWAY_START: &str = "gateway.start";
    pub const GATEWAY_STOP: &str = "gateway.stop";
    pub const GATEWAY_RESTART: &str = "gateway.restart";
    pub const GATEWAY_TRUST: &str = "gateway.trust";
    // ---- 2.1（AI；import.readme 随 README 导入器补） ----
    pub const AI_STATUS: &str = "ai.status";
    pub const AI_COMPLETE: &str = "ai.complete";
    pub const AI_CONFIG_SAVE: &str = "ai.config.save";
    pub const AI_CONFIG_DELETE: &str = "ai.config.delete";
    pub const AI_CONFIG_DEFAULT: &str = "ai.config.default";
    pub const AI_INSTRUCTIONS_SAVE: &str = "ai.instructions.save";
    pub const AI_TEMPLATE_SAVE: &str = "ai.template.save";
    pub const AI_TEMPLATE_DELETE: &str = "ai.template.delete";
    pub const AI_MODELS: &str = "ai.models";
    // ---- 运行页终端（ipc.md §10.15）----
    pub const TERM_OPEN: &str = "term.open";
    pub const TERM_WRITE: &str = "term.write";
    pub const TERM_RESIZE: &str = "term.resize";
    pub const TERM_CLOSE: &str = "term.close";
}

pub mod event {
    // Tauri v2 事件名只允许字母数字与 `-` `/` `:` `_`（点号会被 listen/emit 静默拒绝），
    // 因此用连字符而不是点号；与前端 protocol.ts event 常量保持一致。
    pub const RUNTIME: &str = "st-runtime";
    pub const LOGS: &str = "st-logs";
    pub const METRICS: &str = "st-metrics";
    pub const OPERATION: &str = "st-operation";
    /// 运行页终端输出/退出流（§10.15）。信封 workspace_id 恒为 null（会话是 UI 作用域）。
    pub const TERM: &str = "st-term";
    /// AI complete 流式文本增量（§10.13 扩展）。信封 workspace_id 恒为 null。
    pub const AI: &str = "st-ai";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub protocol: u32,
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
    /// 1.5：结构化错误细节（如 WORKSPACE_LOCKED 的 holder/pid）。additive 字段，
    /// 缺省不序列化；protocol 1 不变，旧前端忽略未知字段。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_yaml::Value>,
}

impl From<&Error> for IpcError {
    fn from(e: &Error) -> Self {
        let Error::App {
            code,
            message,
            retryable,
            details,
        } = e;
        Self {
            protocol: PROTOCOL,
            code: *code,
            message: message.clone(),
            retryable: *retryable,
            details: details.clone(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ServiceId(pub String);

impl ServiceId {
    pub fn parse(raw: &str) -> crate::error::Result<Self> {
        if is_valid_id(raw) {
            Ok(Self(raw.to_string()))
        } else {
            Err(crate::Error::new(
                crate::ErrorCode::SpecInvalid,
                format!("非法 id '{raw}'，需匹配 ^[A-Za-z][A-Za-z0-9_-]{{0,63}}$"),
            ))
        }
    }
}

pub fn is_valid_id(raw: &str) -> bool {
    let b = raw.as_bytes();
    if b.is_empty() || b.len() > 64 {
        return false;
    }
    let first = b[0];
    if !first.is_ascii_alphabetic() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'_' || *c == b'-')
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogSourceKind {
    Service,
    Script,
    System,
    /// 1.6：网关进程日志（id 固定 "gateway"，文件 .supertask/logs/gateway.log）
    Gateway,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogSource {
    pub kind: LogSourceKind,
    pub id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogStream {
    Stdout,
    Stderr,
    System,
}

/// `term.open` 输出：session_id 后续 write/resize/close 及 st.term 事件使用。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermOpenOutput {
    pub session_id: u64,
    /// 实际使用的终端程序（展示用，如 powershell.exe 路径）。
    pub shell: String,
}

/// `st.term` 事件负载（信封外层 { protocol, event, workspace_id: null, ts_ms, payload }）。
/// data 为 lossy UTF-8 终端输出（含 ANSI 序列，前端 xterm 直接渲染）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TermEventPayload {
    pub session_id: u64,
    /// "output" | "exited"
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<String>,
    /// kind = exited 时为退出码（wait 失败为 -1）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
}

impl TermEventPayload {
    pub fn output(session_id: u64, data: String) -> Self {
        Self {
            session_id,
            kind: "output".into(),
            data: Some(data),
            exit_code: None,
        }
    }

    pub fn exited(session_id: u64, exit_code: i32) -> Self {
        Self {
            session_id,
            kind: "exited".into(),
            data: None,
            exit_code: Some(exit_code),
        }
    }
}

/// `st-ai` 事件负载（信封外层 { protocol, event, workspace_id: null, ts_ms, payload }）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiStreamPayload {
    /// 与 `ai.complete` 入参 `request_id` 对应，前端过滤多会话。
    pub request_id: String,
    /// 本块 UTF-8 文本（Markdown 增量）。
    pub delta: String,
}

impl AiStreamPayload {
    pub fn chunk(request_id: impl Into<String>, delta: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            delta: delta.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_rules() {
        assert!(is_valid_id("user-api"));
        assert!(!is_valid_id("1web"));
        assert!(!is_valid_id("../x"));
        assert!(!is_valid_id("a/b"));
    }
}
