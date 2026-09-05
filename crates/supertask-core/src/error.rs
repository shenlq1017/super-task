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
    TemplateIdConflict,
    TemplateParamMissing,
    TemplateParamUnknown,
    TemplateBlockDep,
    TemplateBlockPort,
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
    // ---- 1.2 ----
    ToolchainManagerMissing,
    ToolchainVersionInvalid,
    ToolchainInstallFailed,
    ToolchainPermission,
    PortInUse,
    PortScanFailed,
    PortNoAvailable,
    SecretFileMissing,
    SecretParse,
    SecretMissing,
    SecretGitTracked,
    ProxyInvalid,
    LogQueryInvalid,
    LogExportFailed,
    LogRetentionFailed,
    MetricsUnavailable,
    ProfileNotFound,
    ProfileInvalid,
    ProfileSwitchBusy,
    ProfileDisabled,
    BuildFailed,
    BuildBusy,
    ArtifactMissing,
    JarAmbiguous,
    // ---- 1.3 ----
    DockerNotFound,
    DockerEngineUnreachable,
    DockerComposeMissing,
    ComposeFileMissing,
    ComposeServiceMissing,
    ComposeConfigFailed,
    ComposeUpFailed,
    ComposeStopFailed,
    ComposePortMismatch,
    DockerBuildUnknown,
    ImageBuildFailed,
    // ---- 1.4 ----
    PlatformUnsupported,
    BuildToolAmbiguous,
    GradleWrapperMissing,
    TaskfileNotFound,
    TaskfileInvalid,
    // ---- 1.5 ----
    WorkspaceLocked,
    /// 1.5 CLI `up --wait` 健康等待超时（CLI 层语义，码表与 IPC 同源）
    HealthTimeout,
    PkgNotFound,
    PkgInvalid,
    PkgVersion,
    PkgTargetExists,
    // ---- 1.6 ----
    GatewayNotConfigured,
    GatewayRouteInvalid,
    GatewayBinaryMissing,
    GatewayConfigInvalid,
    GatewayStartFailed,
    // ---- 1.7 ----
    EntryNotFound,
    PackageNotFound,
    // ---- 2.0 ----
    CloudNotLoggedIn,
    CloudAuthFailed,
    CloudOffline,
    CloudSyncConflict,
    CloudEncryptRequired,
    CloudQuotaExceeded,
    CloudProtocolError,
    // ---- 2.1 ----
    AiNotConfigured,
    AiRequestFailed,
    AiTimeout,
    AiContextTooLarge,
    ReadmeNotFound,
    // ---- 运行页终端（ipc.md §10.15）----
    TermSessionNotFound,
    TermSpawnFailed,
    TermLimit,
    // ---- 2.2（方向三·环境供给：声明式 needs，ipc.md §10.17）----
    NeedsInvalid,
    // ---- 2.2（方向六·数据与备份：数据快照，ipc.md §10.18）----
    DataInvalid,
    SnapshotNotFound,
    SnapshotInvalid,
    SnapshotVersion,
    SnapshotBusy,
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

    pub fn message(&self) -> &str {
        match self {
            Self::App { message, .. } => message,
        }
    }

    pub fn soon(since: &str, cmd: &str) -> Self {
        Self::new(ErrorCode::FeatureSoon, format!("{cmd} 将在 {since} 提供"))
            .details(serde_yaml::Value::String(since.to_string()))
    }
}

pub type Result<T> = std::result::Result<T, Error>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v12_error_codes_serialize_screaming_snake() {
        let pairs = [
            (
                ErrorCode::ToolchainManagerMissing,
                "TOOLCHAIN_MANAGER_MISSING",
            ),
            (
                ErrorCode::ToolchainVersionInvalid,
                "TOOLCHAIN_VERSION_INVALID",
            ),
            (
                ErrorCode::ToolchainInstallFailed,
                "TOOLCHAIN_INSTALL_FAILED",
            ),
            (ErrorCode::ToolchainPermission, "TOOLCHAIN_PERMISSION"),
            (ErrorCode::PortInUse, "PORT_IN_USE"),
            (ErrorCode::PortScanFailed, "PORT_SCAN_FAILED"),
            (ErrorCode::PortNoAvailable, "PORT_NO_AVAILABLE"),
            (ErrorCode::SecretFileMissing, "SECRET_FILE_MISSING"),
            (ErrorCode::SecretParse, "SECRET_PARSE"),
            (ErrorCode::SecretMissing, "SECRET_MISSING"),
            (ErrorCode::SecretGitTracked, "SECRET_GIT_TRACKED"),
            (ErrorCode::ProxyInvalid, "PROXY_INVALID"),
            (ErrorCode::LogQueryInvalid, "LOG_QUERY_INVALID"),
            (ErrorCode::LogExportFailed, "LOG_EXPORT_FAILED"),
            (ErrorCode::LogRetentionFailed, "LOG_RETENTION_FAILED"),
            (ErrorCode::MetricsUnavailable, "METRICS_UNAVAILABLE"),
            (ErrorCode::ProfileNotFound, "PROFILE_NOT_FOUND"),
            (ErrorCode::ProfileInvalid, "PROFILE_INVALID"),
            (ErrorCode::ProfileSwitchBusy, "PROFILE_SWITCH_BUSY"),
            (ErrorCode::ProfileDisabled, "PROFILE_DISABLED"),
            (ErrorCode::BuildFailed, "BUILD_FAILED"),
            (ErrorCode::BuildBusy, "BUILD_BUSY"),
            (ErrorCode::ArtifactMissing, "ARTIFACT_MISSING"),
            (ErrorCode::JarAmbiguous, "JAR_AMBIGUOUS"),
            (ErrorCode::PortDup, "PORT_DUP"),
            (ErrorCode::LaunchUnsupported, "LAUNCH_UNSUPPORTED"),
            (ErrorCode::DockerNotFound, "DOCKER_NOT_FOUND"),
            (
                ErrorCode::DockerEngineUnreachable,
                "DOCKER_ENGINE_UNREACHABLE",
            ),
            (ErrorCode::DockerComposeMissing, "DOCKER_COMPOSE_MISSING"),
            (ErrorCode::ComposeFileMissing, "COMPOSE_FILE_MISSING"),
            (ErrorCode::ComposeServiceMissing, "COMPOSE_SERVICE_MISSING"),
            (ErrorCode::ComposeConfigFailed, "COMPOSE_CONFIG_FAILED"),
            (ErrorCode::ComposeUpFailed, "COMPOSE_UP_FAILED"),
            (ErrorCode::ComposeStopFailed, "COMPOSE_STOP_FAILED"),
            (ErrorCode::ComposePortMismatch, "COMPOSE_PORT_MISMATCH"),
            (ErrorCode::DockerBuildUnknown, "DOCKER_BUILD_UNKNOWN"),
            (ErrorCode::ImageBuildFailed, "IMAGE_BUILD_FAILED"),
            (ErrorCode::WorkspaceLocked, "WORKSPACE_LOCKED"),
            (ErrorCode::HealthTimeout, "HEALTH_TIMEOUT"),
            (ErrorCode::PkgNotFound, "PKG_NOT_FOUND"),
            (ErrorCode::PkgInvalid, "PKG_INVALID"),
            (ErrorCode::PkgVersion, "PKG_VERSION"),
            (ErrorCode::PkgTargetExists, "PKG_TARGET_EXISTS"),
            (ErrorCode::GatewayNotConfigured, "GATEWAY_NOT_CONFIGURED"),
            (ErrorCode::GatewayRouteInvalid, "GATEWAY_ROUTE_INVALID"),
            (ErrorCode::GatewayBinaryMissing, "GATEWAY_BINARY_MISSING"),
            (ErrorCode::GatewayConfigInvalid, "GATEWAY_CONFIG_INVALID"),
            (ErrorCode::GatewayStartFailed, "GATEWAY_START_FAILED"),
            // 2.1
            (ErrorCode::ReadmeNotFound, "README_NOT_FOUND"),
            // 终端（§10.15）
            (ErrorCode::TermSessionNotFound, "TERM_SESSION_NOT_FOUND"),
            (ErrorCode::TermSpawnFailed, "TERM_SPAWN_FAILED"),
            (ErrorCode::TermLimit, "TERM_LIMIT"),
            // 声明式 needs（§10.17）
            (ErrorCode::NeedsInvalid, "NEEDS_INVALID"),
            // 数据快照（§10.18）
            (ErrorCode::DataInvalid, "DATA_INVALID"),
            (ErrorCode::SnapshotNotFound, "SNAPSHOT_NOT_FOUND"),
            (ErrorCode::SnapshotInvalid, "SNAPSHOT_INVALID"),
            (ErrorCode::SnapshotVersion, "SNAPSHOT_VERSION"),
            (ErrorCode::SnapshotBusy, "SNAPSHOT_BUSY"),
        ];
        for (code, expected) in pairs {
            let value = serde_yaml::to_value(code).expect("serialize ErrorCode");
            assert_eq!(
                value,
                serde_yaml::Value::String(expected.to_string()),
                "{expected}"
            );
        }
    }
}
