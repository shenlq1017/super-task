mod file;
pub mod validate;

pub use file::{
    DockerBuild, DockerSpec, GatewayConf, GatewayKind, GatewayRoute, GatewayTls, GoNetworkSpec,
    HealthSpec, HealthType, LogRetentionSpec, LoggingSpec, MavenNetworkSpec, NetworkSpec,
    NpmNetworkSpec, PackageManager, ParseWarning, ProfileItem, ProfileServiceOverride,
    ProfilesSpec, ProxyMode, ProxySpec, PythonNetworkSpec, ScriptSpec, SecretsBackend, SecretsSpec,
    ServiceSpec, SuperTaskFile, ToolchainManager, ToolchainSpec, MAX_GROUP_CHARS, MAX_PROFILES,
};
pub use validate::{is_valid_secret_key, is_valid_toolchain_version, validate, validate_proxy_url};

use crate::error::{Error, ErrorCode, Result};
use sha2::{Digest, Sha256};

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
