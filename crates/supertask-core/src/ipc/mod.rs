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
}

pub mod event {
    pub const RUNTIME: &str = "st.runtime";
    pub const LOGS: &str = "st.logs";
    pub const METRICS: &str = "st.metrics";
    pub const OPERATION: &str = "st.operation";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpcError {
    pub protocol: u32,
    pub code: ErrorCode,
    pub message: String,
    pub retryable: bool,
}

impl From<&Error> for IpcError {
    fn from(e: &Error) -> Self {
        let Error::App {
            code,
            message,
            retryable,
            ..
        } = e;
        Self {
            protocol: PROTOCOL,
            code: *code,
            message: message.clone(),
            retryable: *retryable,
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
