use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::{is_valid_id, MAX_CMDS, MAX_ENV_KEYS, MAX_SERVICES};

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
    pub secrets: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub profiles: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub toolchain: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub templates: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub docker: Option<Value>,
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
                        // 缺省 TCP：actuator 检测只在扫描层做（scan.rs 查 pom），
                        // spec 默认值层拿不到 pom；未装 actuator 的应用打
                        // /actuator/health 永远 404，会把运行中的服务误判为不健康。
                        // 需要 HTTP 探测时在 yaml 显式写 health.type: http。
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
            return Err(Error::new(ErrorCode::SpecInvalid, format!("脚本 {id} env 过多")));
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
            return Err(Error::new(ErrorCode::SpecInvalid, format!("服务 {id} env 过多")));
        }
    }
    for id in file.scripts.keys() {
        if !is_valid_id(id) {
            return Err(Error::new(ErrorCode::SpecInvalid, format!("非法脚本 id {id}")));
        }
    }
    Ok(())
}
