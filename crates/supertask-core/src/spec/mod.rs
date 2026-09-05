mod file;
pub mod validate;

pub use file::{
    DataSpec, DataVolumeSpec, DockerBuild, DockerSpec, GatewayConf, GatewayCorsSpec, GatewayKind,
    GatewayRoute, GatewayTls, GoNetworkSpec, HealthSpec, HealthType, LogRetentionSpec, LoggingSpec,
    MavenNetworkSpec, NetworkSpec, NpmNetworkSpec, PackageManager, ParseWarning, ProfileItem,
    ProfileServiceOverride, ProfilesSpec, ProxyMode, ProxySpec, PythonNetworkSpec, ScriptSpec,
    SecretsBackend, SecretsSpec, ServiceSpec, SuperTaskFile, ToolchainManager, ToolchainSpec,
    MAX_DATA_VOLUMES, MAX_GROUP_CHARS, MAX_PROFILES,
};
pub use validate::{is_valid_secret_key, is_valid_toolchain_version, validate, validate_proxy_url};

use crate::error::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};

// ---------------------------------------------------------------------------
// 2.2 `restart` 策略：进程意外退出后的自动重启监管（方向一·服务监管）
// ---------------------------------------------------------------------------

/// `restart` 取值。不设 `unless-stopped`：引擎生命周期即应用会话，
/// 会话内它与 `always` 行为一致，不引入同名异义的两个值。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RestartPolicy {
    #[default]
    Never,
    OnFailure,
    Always,
}

impl RestartPolicy {
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "never" => Some(Self::Never),
            "on-failure" => Some(Self::OnFailure),
            "always" => Some(Self::Always),
            _ => None,
        }
    }
}

/// 自动重试上限缺省值与允许上限（静态校验保证 1..=100）。
pub const RESTART_MAX_RETRIES_DEFAULT: u32 = 5;
pub const RESTART_MAX_RETRIES_MAX: u32 = 100;

/// spawn 时随启动计划下发的 restart 解析结果。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartSpec {
    pub policy: RestartPolicy,
    pub max_retries: u32,
}

impl Default for RestartSpec {
    fn default() -> Self {
        Self {
            policy: RestartPolicy::Never,
            max_retries: RESTART_MAX_RETRIES_DEFAULT,
        }
    }
}

/// spawn 时的防坑重解析：静态校验已保证组合合法，这里对非法/缺省值回落 never。
pub fn resolve_restart(svc: &ServiceSpec) -> RestartSpec {
    let policy = svc
        .restart
        .as_deref()
        .and_then(RestartPolicy::parse)
        .unwrap_or(RestartPolicy::Never);
    RestartSpec {
        policy,
        max_retries: svc.max_retries.unwrap_or(RESTART_MAX_RETRIES_DEFAULT),
    }
}

pub fn parse_yaml(text: &str) -> Result<(SuperTaskFile, Vec<ParseWarning>)> {
    if text.len() > crate::ipc::MAX_YAML_BYTES {
        return Err(Error::new(
            ErrorCode::YamlTooLarge,
            format!("YAML 超过 {} 字节上限", crate::ipc::MAX_YAML_BYTES),
        ));
    }
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut file: SuperTaskFile = serde_yaml::from_str(stripped).map_err(|e| {
        let line = e.location().map(|l| l.line());
        let mut err = Error::new(ErrorCode::YamlParse, format!("YAML 解析失败: {e}"));
        if let Some(line) = line {
            err = err.details(serde_yaml::to_value(line).unwrap_or(serde_yaml::Value::Null));
        }
        err
    })?;
    file.apply_defaults();
    let warnings = validate(&file)?;
    Ok((file, warnings))
}

pub fn spec_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    hex_lower(&h.finalize())
}

fn hex_lower(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0xf) as usize] as char);
    }
    s
}

pub fn to_yaml(file: &SuperTaskFile) -> Result<String> {
    serde_yaml::to_string(file).map_err(|e| Error::new(ErrorCode::SpecInvalid, e.to_string()))
}
