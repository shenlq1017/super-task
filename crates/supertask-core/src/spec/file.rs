use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::{is_valid_id, MAX_CMDS, MAX_ENV_KEYS, MAX_SERVICES};

/// 1.2: profile 数量上限（规格 §10.1）。
pub const MAX_PROFILES: usize = 32;
/// 1.2: services.*.group 显示名最长字符数。
pub const MAX_GROUP_CHARS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseWarning {
    pub code: ErrorCode,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuperTaskFile {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default = "default_root")]
    pub root: String,
    #[serde(default, deserialize_with = "de_env")]
    pub env: IndexMap<String, String>,
    pub services: IndexMap<String, ServiceSpec>,
    #[serde(default)]
    pub scripts: IndexMap<String, ScriptSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub secrets: Option<SecretsSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<ProfilesSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<ToolchainSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub network: Option<NetworkSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub log_retention: Option<LogRetentionSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<DockerSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cloud: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ai: Option<Value>,
    /// Unknown / `x-*` top-level keys. Round-tripped on form save.
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

fn default_root() -> String {
    ".".into()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServiceSpec {
    pub kind: String,
    /// 1.3 `kind: compose`：compose 文件内的服务名（非 SuperTask id）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub labels: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ports: Vec<u16>,
    #[serde(default, deserialize_with = "de_env")]
    pub env: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub env_file: Vec<String>,
    #[serde(default)]
    pub depends_on: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depends_on_ex: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grace_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<HealthSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub extra_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub build_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub launch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub module: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jvm_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub logging: Option<LoggingSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    Npm,
    Pnpm,
    Yarn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolchainManager {
    Auto,
    Mise,
    Winget,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolchainSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manager: Option<ToolchainManager>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub java: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maven: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub node: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SecretsBackend {
    File,
    Env,
    /// 1.0 sample alias: file + default `.env.local`.
    Local,
}

impl SecretsBackend {
    pub fn is_file(self) -> bool {
        matches!(self, Self::File | Self::Local)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecretsSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backend: Option<SecretsBackend>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required: Vec<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ProxyMode {
    Off,
    System,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxySpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<ProxyMode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub https: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub no_proxy: Vec<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenNetworkSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mirror: Option<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NpmNetworkSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub registry: Option<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub proxy: Option<ProxySpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub maven: Option<MavenNetworkSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub npm: Option<NpmNetworkSpec>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

/// 1.3 `docker.builds` 条目。context/dockerfile 相对 root；tag 格式见 validate。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerBuild {
    pub name: String,
    pub context: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dockerfile: Option<String>,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

/// 1.3 顶层 `docker` 段（typed）。compose 文件仍是容器行为唯一真源，
/// 这里只存 SuperTask 需要的引用与构建入口。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DockerSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compose_file: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_name: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub builds: Vec<DockerBuild>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileServiceOverride {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
    #[serde(
        default,
        deserialize_with = "de_env",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub env: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfileItem {
    #[serde(
        default,
        deserialize_with = "de_env",
        skip_serializing_if = "IndexMap::is_empty"
    )]
    pub env: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub services: IndexMap<String, ProfileServiceOverride>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilesSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub items: IndexMap<String, ProfileItem>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogRetentionSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_files: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_days: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_total_bytes: Option<u64>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthSpec {
    #[serde(rename = "type", default)]
    pub r#type: HealthType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub http: Option<String>,
    #[serde(default = "default_interval")]
    pub interval_secs: u32,
    #[serde(default = "default_timeout")]
    pub timeout_secs: u32,
}

fn default_interval() -> u32 {
    2
}
fn default_timeout() -> u32 {
    2
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum HealthType {
    #[default]
    None,
    Tcp,
    Http,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub desc: Option<String>,
    pub cmds: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, deserialize_with = "de_env")]
    pub env: IndexMap<String, String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timeout_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub depends_on: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ring_lines: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retain_tail_bytes: Option<u64>,
}

impl SuperTaskFile {
    pub fn apply_defaults(&mut self) {
        if self.root.is_empty() {
            self.root = ".".into();
        }
        for (id, svc) in self.services.iter_mut() {
            let _ = id;
            match svc.kind.as_str() {
                "spring-boot" => {
                    if svc.grace_secs.is_none() {
                        svc.grace_secs = Some(45);
                    }
                    if svc.launch.is_none() {
                        svc.launch = Some("run".into());
                    }
                    if svc.health.is_none() {
                        svc.health = Some(HealthSpec {
                            r#type: HealthType::Tcp,
                            http: None,
                            interval_secs: 2,
                            timeout_secs: 2,
                        });
                    }
                }
                "node" => {
                    if svc.grace_secs.is_none() {
                        svc.grace_secs = Some(15);
                    }
                    if svc.health.is_none() {
                        svc.health = Some(HealthSpec {
                            r#type: HealthType::Tcp,
                            http: None,
                            interval_secs: 2,
                            timeout_secs: 2,
                        });
                    }
                }
                _ => {}
            }
        }
        for s in self.scripts.values_mut() {
            if s.timeout_secs.is_none() {
                s.timeout_secs = Some(1800);
            }
        }
    }

    pub fn runnable_kind(kind: &str) -> bool {
        matches!(kind, "spring-boot" | "node")
    }
}

fn de_env<'de, D>(deserializer: D) -> std::result::Result<IndexMap<String, String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let raw: IndexMap<String, Value> =
        Option::<IndexMap<String, Value>>::deserialize(deserializer)?.unwrap_or_default();
    let mut out = IndexMap::new();
    for (k, v) in raw {
        out.insert(k, value_to_string(&v).map_err(serde::de::Error::custom)?);
    }
    Ok(out)
}

fn value_to_string(v: &Value) -> Result<String> {
    match v {
        Value::Null => Ok(String::new()),
        Value::Bool(b) => Ok(b.to_string()),
        Value::Number(n) => Ok(n.to_string()),
        Value::String(s) => Ok(s.clone()),
        _ => Err(Error::new(ErrorCode::SpecInvalid, "env 值必须是标量")),
    }
}

pub fn check_limits(file: &SuperTaskFile) -> Result<()> {
    if file.services.len() > MAX_SERVICES {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            format!("服务数超过 {MAX_SERVICES}"),
        ));
    }
    if file.env.len() > MAX_ENV_KEYS {
        return Err(Error::new(ErrorCode::SpecInvalid, "工作区 env 键过多"));
    }
    for (id, s) in &file.scripts {
        if s.cmds.len() > MAX_CMDS || s.cmds.is_empty() {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("脚本 {id} 的 cmds 须为 1..{MAX_CMDS} 条"),
            ));
        }
        if s.env.len() > MAX_ENV_KEYS {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("脚本 {id} env 过多"),
            ));
        }
    }
    for (id, svc) in &file.services {
        if !is_valid_id(id) {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("非法服务 id {id}"),
            ));
        }
        if svc.env.len() > MAX_ENV_KEYS {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("服务 {id} env 过多"),
            ));
        }
    }
    for id in file.scripts.keys() {
        if !is_valid_id(id) {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("非法脚本 id {id}"),
            ));
        }
    }
    Ok(())
}
