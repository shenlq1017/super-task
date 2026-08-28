use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    Protocol,
    FeatureSoon,
    FeatureDisabled,
    NoWorkspace,
    NotFound,
    NoYaml,
    YamlParse,
    YamlDupFile,
    YamlTooLarge,
    YamlConflict,
    SpecInvalid,
    SpecNewer,
    KindUnsupported,
    LaunchUnsupported,
    Cycle,
    MissingTool,
    CwdMissing,
    PathEscape,
    HealthHostForbidden,
    Spawn,
    AlreadyInProgress,
    DepDead,
    JobKill,
    JobCreate,
    ScriptBusy,
    PortDup,
    Discover,
    // ---- 1.1 ----
    TargetNotEmpty,
    TemplateInvalid,
    TemplateWrite,
    GitNotFound,
    GitNotRepository,
    GitDirty,
    GitWorkspaceBusy,
    GitAuth,
    GitRemote,
    GitBranch,
    GitConflict,
    GitFailed,
    IdeNotFound,
    AutostartFailed,
    UpdateBlockedRunning,
    UpdateSignature,
    UpdateFailed,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("{message}")]
    App {
        code: ErrorCode,
        message: String,
        retryable: bool,
        details: Option<serde_yaml::Value>,
    },
}

impl Error {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self::App {
            code,
            message: message.into(),
            retryable: false,
            details: None,
        }
    }

    pub fn retryable(self) -> Self {
        let Self::App {
            code,
            message,
            details,
            ..
        } = self;
        Self::App {
            code,
            message,
            retryable: true,
            details,
        }
    }

    pub fn details(self, details: serde_yaml::Value) -> Self {
        let Self::App {
            code,
            message,
            retryable,
            ..
        } = self;
        Self::App {
            code,
            message,
            retryable,
            details: Some(details),
        }
    }

    pub fn code(&self) -> ErrorCode {
        match self {
            Self::App { code, .. } => *code,
        }
    }

    pub fn soon(since: &str, cmd: &str) -> Self {
        Self::new(
            ErrorCode::FeatureSoon,
            format!("{cmd} 将在 {since} 提供"),
        )
        .details(serde_yaml::Value::String(since.to_string()))
    }
}

pub type Result<T> = std::result::Result<T, Error>;
