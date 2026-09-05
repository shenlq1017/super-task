use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::{is_valid_id, MAX_CMDS, MAX_ENV_KEYS, MAX_SERVICES};

/// 1.2: profile 数量上限（规格 §10.1）。
pub const MAX_PROFILES: usize = 32;
/// 方向三：声明式 needs 条目上限。
pub const MAX_NEEDS: usize = 32;
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
    /// 方向三·环境供给：声明式需求，项形如 `node@20` / `postgres@16`（yaml.md §7.2）。
    /// 仅作为 `workspace.needsResolve` 的解析输入，不影响加载与启动行为。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs: Option<Vec<String>>,
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
    /// 1.6：网关段转正（typed）。`gateway: {}`（1.0 reserved）语义不变：
    /// kind 为 None 即「未配置」，读回仍在、不产生行为。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<GatewayConf>,
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
    /// 2.2：自动重启次数上限（1..=100），仅 `restart: on-failure|always` 时有意义；缺省 5。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_retries: Option<u32>,
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
    /// 1.4 §5.1：`maven | gradle`；缺省按构建文件探测（并存 → BUILD_TOOL_AMBIGUOUS）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub build_tool: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub jvm_args: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_manager: Option<PackageManager>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub script: Option<String>,
    /// 1.7 `kind: python`：脚本入口（相对 dir），与 module 恰一。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub entry: Option<String>,
    /// 1.7 `kind: go`：`go run` 的包路径（相对 dir），缺省 "."。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
    /// 1.7 `kind: generic`：程序名（PATH 解析）或工作区内相对路径（含路径分隔符时）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub program: Option<String>,
    /// 1.7 `kind: generic`：程序参数（extra_args 仍追加在其后）。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
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
    /// Bun projects may be used as a Node service or by Maven frontend modules.
    Bun,
}

// ---------------------------------------------------------------------------
// 1.6 顶层 `gateway:` 段（typed，规格 §4.1）
// ---------------------------------------------------------------------------

/// 反代引擎：nginx 一等公民；caddy 本机 HTTPS；apache 最小反代集。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GatewayKind {
    Nginx,
    Caddy,
    Apache,
}

impl GatewayKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Nginx => "nginx",
            Self::Caddy => "caddy",
            Self::Apache => "apache",
        }
    }
}

/// 1.6 `tls`：仅 caddy 生效；internal = Caddy 内置 local CA 的本机 HTTPS。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum GatewayTls {
    #[default]
    Off,
    Internal,
}

/// route 级 CORS（方向四，仅代理路由）。`origins` 必填：`*` 或
/// `http(s)://host[:port]`（不可与其他项混用 `*`）；methods/headers 缺省取
/// 常规开发集；credentials=true 时拒绝 `*`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayCorsSpec {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub origins: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub methods: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headers: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_age_secs: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<bool>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

/// 单条路由（方向四起共三种形态，恰选其一）：
/// - 代理：`target`/`upstream` 二选一（可带 `strip_prefix` 剥前缀、`cors`）；
/// - 重定向：`redirect` 目标（`/path` 或 `http(s)://…`），可选 `redirect_status`
///   （301/302/307/308，缺省 302）；
/// - 静态站点：`static_dir`（工作区内相对目录，`path` 必须为 `/`）。
/// `host` 支持逗号分隔多域名别名（空 = 全匹配 catch-all）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRoute {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub strip_prefix: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cors: Option<GatewayCorsSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub redirect_status: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub static_dir: Option<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

/// 1.6 顶层 `gateway:` 段。缺省字段序列化时跳过，`gateway: {}` round-trip 后
/// 仍是 `{}`（未配置语义不变）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayConf {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<GatewayKind>,
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub enabled: bool,
    #[serde(
        default = "default_gateway_port",
        skip_serializing_if = "is_default_gateway_port"
    )]
    pub port: u16,
    /// 二进制显式路径（探测的最终 fallback；PATH_ESCAPE 不适用——这是绝对路径值）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bin: Option<String>,
    #[serde(default, skip_serializing_if = "GatewayTls::is_off")]
    pub tls: GatewayTls,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub routes: Vec<GatewayRoute>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

fn default_gateway_port() -> u16 {
    8080
}

fn is_true(b: &bool) -> bool {
    *b
}

fn is_default_gateway_port(p: &u16) -> bool {
    *p == 8080
}

impl GatewayTls {
    pub fn is_off(tls: &Self) -> bool {
        *tls == Self::Off
    }
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
    /// 1.7：`major.minor`（如 "3.12" / "1.23"），钉扎语义同 java/node。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go: Option<String>,
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
/// 1.7：`pip` 镜像（运行时注入 `PIP_INDEX_URL`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonNetworkSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index_url: Option<String>,
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}
/// 1.7：Go 模块代理（运行时注入 `GOPROXY`）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoNetworkSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub goproxy: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub python: Option<PythonNetworkSpec>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub go: Option<GoNetworkSpec>,
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
                // 1.7 §4.4：python 15 / go 60（冷编译宽限）/ generic 15；
                // health 默认 tcp 仅在有 port 时（无端口服务健康只能 none）。
                "python" | "go" | "generic" => {
                    let default_grace = if svc.kind == "go" { 60 } else { 15 };
                    if svc.grace_secs.is_none() {
                        svc.grace_secs = Some(default_grace);
                    }
                    if svc.health.is_none() && svc.port.is_some() {
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
        matches!(kind, "spring-boot" | "node" | "python" | "go" | "generic")
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
