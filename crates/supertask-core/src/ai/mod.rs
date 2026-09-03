//! 2.1 AI 助手（v2.1 规格 §4 + 截图对齐升级）：provider 配置 + key 存取 + 三场景 complete。
//!
//! - 配置：appdata **命名多配置**（`ai_configs`，name 唯一 + 默认配置；旧单配置 `ai`
//!   字段作为迁移来源，见 [`configs`]）；key 存应用级 secrets 文件固定 id
//!   `SUPERTASK_AI`（逻辑 id `supertask.ai`，backend=local，沿 1.2 dotenv 基建；
//!   **永不入云 vault，也不出现在任何返回值/日志**，v2.1 规格 §4.1）。
//! - 传输：HTTP 复用 v2.0 ureq；[`AiHttp`] trait + fake 注入，单测零真实网络；
//!   api_style 支持 OpenAI 兼容 chat/completions 与 Anthropic Messages（client.rs）。
//! - 数据卫生（spec §4.3）：进 prompt 前掩码、字符 ÷4 粗估超限报 `AI_CONTEXT_TOO_LARGE`、
//!   按日用量计数（AppData.aiUsage）；**偏差备案**：spec 原文「无自动重试」放宽为
//!   「仅临时错误（限流/超时/网络）自动重试 ≤ max_retries（默认 2），全部尝试计入用量」。
//! - 全局自定义指令（≤8000 字符）与场景 Prompt 模板（enabled 总量 ≤16000 字符）
//!   注入 system（限额沿 dbx prompt_template）。

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::appdata::AppData;
use crate::error::{Error, ErrorCode, Result};

pub mod cli_agent;
pub mod client;
pub mod prompt;
pub mod sanitize;

pub use client::{
    normalize_proxy_url, parse_chat_response, parse_models_response, redact_key, AiHttp,
    AiHttpResponse, TokenUsage, UreqAiHttp,
};
pub use prompt::{
    build_config_suggest, build_enrich_draft, build_explain_logs, build_test_connection,
    ConfigSuggestInput, ExplainLogsInput, ServiceContext,
};
pub use sanitize::{sanitize_text, tail_truncate, REDACTED};

/// key 的逻辑 id（spec §4.1 固定 id）；磁盘文件里映射为合法 dotenv key [`AI_SECRET_KEY`]。
pub const AI_SECRET_ID: &str = "supertask.ai";
/// dotenv key（1.2 secrets 基建要求 `[A-Za-z_][A-Za-z0-9_]*`，不带点）。
pub const AI_SECRET_KEY: &str = "SUPERTASK_AI";
/// 应用级 secrets 文件名（appdata 目录下；workspace `.env.local` 互不相干）。
pub const AI_SECRET_FILE: &str = "secrets.env";
/// 默认请求超时（秒）。
pub const DEFAULT_TIMEOUT_SECS: u64 = 120;
/// 默认 max_tokens（日志解释等 markdown 场景需要更长输出）。
pub const DEFAULT_MAX_TOKENS: u32 = 8192;
/// 默认最大重试次数（仅临时错误；偏差备案见模块注释）。
pub const DEFAULT_MAX_RETRIES: u32 = 2;
/// 无 context_window 时的上下文预算（字符 ÷4 粗估 token，spec §4.3）。
pub const MAX_CONTEXT_TOKENS: usize = 24_000;
/// 全局自定义指令上限（字符）。
pub const GLOBAL_INSTRUCTIONS_LIMIT: usize = 8_000;
/// 单个模板内容上限（字符）。
pub const TEMPLATE_CONTENT_LIMIT: usize = 8_000;
/// 模板名上限（字符）。
pub const TEMPLATE_NAME_LIMIT: usize = 50;
/// 激活模板内容总量上限（字符）。
pub const TEMPLATES_ACTIVE_TOTAL_LIMIT: usize = 16_000;

/// API 风格（dbx `AiApiStyle` 裁剪到两个 API 体系；CLI provider 不做）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApiStyle {
    OpenAiCompletions,
    AnthropicMessages,
}

/// 认证方式（dbx `AiAuthMethod`）：api-key 按风格映射 Bearer / x-api-key；bearer 恒 Bearer。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthMethod {
    #[default]
    ApiKey,
    Bearer,
}

/// provider 预设（前端下拉同源；default_endpoint/default_model 供 UI 预填）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProviderPreset {
    pub key: &'static str,
    pub api_style: ApiStyle,
    /// 本地/免鉴权 provider 允许不设 key（dbx validate_config：兼容/本地允许空 key）。
    pub key_optional: bool,
    pub default_endpoint: &'static str,
    pub default_model: &'static str,
    /// HTTP 端点还是本机 CLI；决定校验规则与执行路径。
    pub kind: ProviderKind,
    /// 本地 CLI 的默认可执行名（PATH 查找）；HTTP provider 为空。
    pub cli_program: &'static str,
    /// 本地 CLI 的默认参数。取自 dbx 的实现，但配置里可改：各家 flag 会随版本变，
    /// 写死会让用户在 CLI 升级后无路可走。
    pub cli_args: &'static [&'static str],
    /// 本地 CLI 常见可选模型（CLI 无 /models 端点，模型发现只能给预置项）。
    pub cli_models: &'static [&'static str],
}

/// provider 的执行形态。
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// 通过 HTTP 调远端（或本地 Ollama）端点。
    #[default]
    Http,
    /// 调用本机已安装的编码 CLI，凭据由该 CLI 自己管。
    LocalCli,
}

/// HTTP provider 的预设简写（保持既有条目可读）。
const fn http(
    key: &'static str,
    api_style: ApiStyle,
    key_optional: bool,
    default_endpoint: &'static str,
    default_model: &'static str,
) -> ProviderPreset {
    ProviderPreset {
        key,
        api_style,
        key_optional,
        default_endpoint,
        default_model,
        kind: ProviderKind::Http,
        cli_program: "",
        cli_args: &[],
        cli_models: &[],
    }
}

/// 本地 CLI provider 的预设简写。key 恒为可选（凭据在 CLI 侧）。
const fn local_cli(
    key: &'static str,
    cli_program: &'static str,
    cli_args: &'static [&'static str],
    cli_models: &'static [&'static str],
) -> ProviderPreset {
    ProviderPreset {
        key,
        api_style: ApiStyle::OpenAiCompletions,
        key_optional: true,
        default_endpoint: "",
        default_model: "default",
        kind: ProviderKind::LocalCli,
        cli_program,
        cli_args,
        cli_models,
    }
}

/// 与前端 `AI_PROVIDER_PRESETS` 同源。
///
/// 两类：HTTP 端点，以及本机编码 CLI（`*-cli`）。CLI 的默认 argv 参考 dbx 的对应
/// 实现，选的都是各家「非交互一次性输出」模式；用户可在配置里覆盖。
pub const PROVIDER_PRESETS: &[ProviderPreset] = &[
    http(
        "openai-compatible",
        ApiStyle::OpenAiCompletions,
        false,
        "https://api.openai.com/v1",
        "gpt-4o-mini",
    ),
    http(
        "claude",
        ApiStyle::AnthropicMessages,
        false,
        "https://api.anthropic.com",
        "claude-sonnet-4-5",
    ),
    http(
        "anthropic-compatible",
        ApiStyle::AnthropicMessages,
        true,
        "",
        "",
    ),
    http(
        "deepseek",
        ApiStyle::OpenAiCompletions,
        false,
        "https://api.deepseek.com",
        "deepseek-chat",
    ),
    http(
        "kimi",
        ApiStyle::OpenAiCompletions,
        false,
        "https://api.moonshot.cn/v1",
        "kimi-k2-0905-preview",
    ),
    http(
        "qwen",
        ApiStyle::OpenAiCompletions,
        false,
        "https://dashscope.aliyuncs.com/compatible-mode/v1",
        "qwen-plus",
    ),
    http(
        "minimax",
        ApiStyle::OpenAiCompletions,
        false,
        "https://api.minimaxi.com/v1",
        "MiniMax-Text-01",
    ),
    http(
        "gemini",
        ApiStyle::OpenAiCompletions,
        false,
        "https://generativelanguage.googleapis.com/v1beta/openai",
        "gemini-2.0-flash",
    ),
    http(
        "ollama",
        ApiStyle::OpenAiCompletions,
        true,
        "http://localhost:11434/v1",
        "qwen2.5:7b",
    ),
    // ---- 本地 CLI ----
    local_cli(
        "claude-code-cli",
        "claude",
        &[
            "--print",
            "--output-format",
            "stream-json",
            "--verbose",
            "--input-format",
            "text",
            "--no-session-persistence",
            "--permission-mode",
            "dontAsk",
            "--tools",
            "",
        ],
        &["default", "sonnet", "opus", "haiku"],
    ),
    local_cli(
        "codex-cli",
        "codex",
        &[
            "exec",
            "--json",
            "--skip-git-repo-check",
            "--sandbox",
            "read-only",
            "-c",
            "features.shell_tool=false",
            "-c",
            "web_search=\"disabled\"",
            "-",
        ],
        &["default", "gpt-5-codex", "o4-mini"],
    ),
    local_cli(
        "opencode-cli",
        "opencode",
        &["run", "--format", "json", "--pure"],
        &["default"],
    ),
    local_cli(
        "cursor-cli",
        "agent",
        &["--print", "--output-format", "text"],
        &["default"],
    ),
    local_cli(
        "codebuddy-cli",
        "codebuddy",
        &["--print", "--output-format", "stream-json", "--verbose"],
        &["default"],
    ),
    local_cli(
        "qoder-cli",
        "qodercli",
        &["--print", "--output-format", "stream-json"],
        &["default"],
    ),
    local_cli("pi-agent-cli", "pi", &["--print"], &["default"]),
    http("custom", ApiStyle::OpenAiCompletions, false, "", ""),
];

pub fn provider_preset(key: &str) -> Option<&'static ProviderPreset> {
    PROVIDER_PRESETS.iter().find(|p| p.key == key)
}

/// provider 配置本体（appdata；camelCase 与 AppData 其余字段一致；新字段带默认值以兼容旧档）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct ProviderConfig {
    pub base_url: String,
    pub model: String,
    pub timeout_secs: u64,
    pub max_tokens: u32,
    /// 预设 key（[`PROVIDER_PRESETS`]）；custom 之外的预设决定 api_style。
    pub provider: String,
    /// 覆盖 api_style（缺省由 provider 预设决定）。
    pub api_style: Option<ApiStyle>,
    pub auth_method: AuthMethod,
    pub proxy_enabled: bool,
    pub proxy_url: Option<String>,
    /// 模型上下文窗口（tokens）；参与预算估算（未设则 [`MAX_CONTEXT_TOKENS`]）。
    pub context_window: Option<u64>,
    /// 临时错误自动重试次数（0–10）。
    pub max_retries: u32,
    /// 本地 CLI provider：可执行文件路径（空 = 走 PATH 查找预设程序名）。
    pub cli_path: Option<String>,
    /// 本地 CLI provider：argv（空 = 用预设默认参数）。
    #[serde(default)]
    pub cli_args: Vec<String>,
    /// 本地 CLI provider：显式传给子进程的环境变量（如 HTTPS_PROXY）。
    #[serde(default)]
    pub cli_env: BTreeMap<String, String>,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            base_url: String::new(),
            model: String::new(),
            timeout_secs: DEFAULT_TIMEOUT_SECS,
            max_tokens: DEFAULT_MAX_TOKENS,
            provider: "openai-compatible".to_string(),
            api_style: None,
            auth_method: AuthMethod::ApiKey,
            proxy_enabled: false,
            proxy_url: None,
            context_window: None,
            max_retries: DEFAULT_MAX_RETRIES,
            cli_path: None,
            cli_args: Vec::new(),
            cli_env: BTreeMap::new(),
        }
    }
}

impl ProviderConfig {
    /// 生效 api_style：显式覆盖 > provider 预设 > OpenAI 兼容。
    pub fn effective_api_style(&self) -> ApiStyle {
        self.api_style
            .or_else(|| provider_preset(&self.provider).map(|p| p.api_style))
            .unwrap_or(ApiStyle::OpenAiCompletions)
    }

    pub fn key_optional(&self) -> bool {
        provider_preset(&self.provider).is_some_and(|p| p.key_optional)
    }

    pub fn kind(&self) -> ProviderKind {
        provider_preset(&self.provider)
            .map(|p| p.kind)
            .unwrap_or_default()
    }

    pub fn is_local_cli(&self) -> bool {
        self.kind() == ProviderKind::LocalCli
    }

    /// 生效 argv：配置里非空则用它，否则用预设默认值。
    pub fn effective_cli_args(&self) -> Vec<String> {
        if !self.cli_args.is_empty() {
            return self.cli_args.clone();
        }
        provider_preset(&self.provider)
            .map(|p| p.cli_args.iter().map(|a| a.to_string()).collect())
            .unwrap_or_default()
    }

    pub fn cli_program(&self) -> String {
        let default_program = provider_preset(&self.provider)
            .map(|p| p.cli_program)
            .unwrap_or("");
        cli_agent::resolve_program(self.cli_path.as_deref(), default_program)
    }
}

/// 命名配置（dbx `ai_configs` 的文件存储版：id + 唯一 name + 配置本体）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NamedAiConfig {
    pub id: String,
    pub name: String,
    pub config: ProviderConfig,
}

/// 场景 Prompt 模板（限额沿 dbx：name 50 / content 8000 / 激活总量 16000，Unicode 字符计数）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiPromptTemplate {
    pub id: String,
    pub name: String,
    pub content: String,
    pub enabled: bool,
}

fn char_len(s: &str) -> usize {
    s.chars().count()
}

fn new_id(prefix: &str) -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{prefix}{nanos:x}")
}

/// 按日用量计数（appdata `aiUsage`；只在用户触发的 complete 尝试后 +1）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsage {
    pub date: String,
    pub count: u64,
}

impl AiUsage {
    pub fn today_count(&self) -> u64 {
        if self.date == today_utc() {
            self.count
        } else {
            0
        }
    }
}

/// base_url 校验：绝对 http/https、无空白/userinfo/query/fragment；
/// 允许路径（OpenAI 兼容端点常见 `http://host:port/v1`）。返回去掉尾部 `/` 的归一值。
pub fn validate_base_url(base_url: &str) -> Result<String> {
    let base_url = base_url.trim();
    if base_url.is_empty() || base_url.chars().any(char::is_whitespace) {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "base_url 不能为空或含空白",
        ));
    }
    let (scheme, rest) = base_url
        .split_once("://")
        .ok_or_else(|| Error::new(ErrorCode::AiNotConfigured, "base_url 只允许 http/https"))?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "base_url 只允许 http/https",
        ));
    }
    if rest.is_empty() || rest.starts_with('/') {
        return Err(Error::new(ErrorCode::AiNotConfigured, "base_url 缺少主机"));
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() || authority.contains('@') || authority.ends_with(':') {
        return Err(Error::new(ErrorCode::AiNotConfigured, "base_url 主机无效"));
    }
    // host[:port]：有端口时必须为数字（IPv6 字面量放行到 host 含 ':' 的分支之外）
    if !authority.starts_with('[') {
        if let Some((host, port)) = authority.rsplit_once(':') {
            if host.is_empty()
                || host.contains(':')
                || port.is_empty()
                || !port.chars().all(|c| c.is_ascii_digit())
            {
                return Err(Error::new(ErrorCode::AiNotConfigured, "base_url 主机无效"));
            }
        }
    }
    if rest[authority_end..].contains('?') || rest[authority_end..].contains('#') {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "base_url 不允许 query 或 fragment",
        ));
    }
    Ok(base_url.trim_end_matches('/').to_string())
}

// ---------------------------------------------------------------------------
// 命名多配置：视图（含旧单配置迁移）与 CRUD
// ---------------------------------------------------------------------------

/// 配置视图：`ai_configs` 非空即真源；否则旧 `ai` 单配置迁移为 id `default`（只读视图，
/// 首次保存时才真正落入 `ai_configs`）。
pub fn configs(app: &AppData) -> Vec<NamedAiConfig> {
    if !app.ai_configs.is_empty() {
        return app.ai_configs.clone();
    }
    match &app.ai {
        Some(cfg) => vec![NamedAiConfig {
            id: "default".to_string(),
            name: "default".to_string(),
            config: cfg.clone(),
        }],
        None => Vec::new(),
    }
}

/// 默认配置：`ai_default_config` 指向者 > 首个；无配置返回 None。
pub fn default_config(app: &AppData) -> Option<NamedAiConfig> {
    let list = configs(app);
    let default_id = app.ai_default_config.as_deref();
    list.iter()
        .find(|c| Some(c.id.as_str()) == default_id)
        .or_else(|| list.first())
        .cloned()
}

/// 首个配置自动成为默认（dbx 唯一 default 语义）。
fn ensure_default(app: &mut AppData) {
    let list = configs(app);
    let ids: Vec<&str> = list.iter().map(|c| c.id.as_str()).collect();
    if !ids.is_empty() && !ids.contains(&app.ai_default_config.as_deref().unwrap_or("")) {
        app.ai_default_config = Some(ids[0].to_string());
    }
}

/// 保存（新建或原位更新）命名配置。`name` 唯一（大小写不敏感）；校验同单配置。
/// 旧 `ai` 单配置在首次保存时迁入 `ai_configs` 后清除（新真源唯一化）。
pub fn config_save(app: &mut AppData, input: ConfigSaveInput) -> Result<NamedAiConfig> {
    let name = input.name.trim().to_string();
    if name.is_empty() || char_len(&name) > TEMPLATE_NAME_LIMIT {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "配置名不能为空且不超过 50 字符",
        ));
    }
    if !input.provider.is_empty() && provider_preset(&input.provider).is_none() {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            format!("未知 provider: {}", input.provider),
        ));
    }
    let local_cli =
        provider_preset(&input.provider).is_some_and(|p| p.kind == ProviderKind::LocalCli);
    // 本地 CLI 没有端点可填，强求 base_url 只会逼用户编一个 URL
    let base_url = if local_cli {
        String::new()
    } else {
        validate_base_url(
            &input
                .base_url
                .clone()
                .unwrap_or_default()
                .trim()
                .to_string(),
        )
        .map_err(|e| {
            if input.base_url.is_none() {
                Error::new(ErrorCode::AiNotConfigured, "base_url 不能为空")
            } else {
                e
            }
        })?
    };
    let model = input.model.trim().to_string();
    if model.is_empty() {
        return Err(Error::new(ErrorCode::AiNotConfigured, "model 不能为空"));
    }
    let cli_path = cli_agent::validate_cli_path(input.cli_path.as_deref().unwrap_or_default())?;
    for name in input.cli_env.keys() {
        cli_agent::validate_env_name(name)?;
    }
    if input.proxy_enabled {
        normalize_proxy_url(input.proxy_url.as_deref().unwrap_or_default())?;
    }

    // 旧单配置首次落库
    if app.ai_configs.is_empty() {
        if let Some(legacy) = app.ai.take() {
            app.ai_configs.push(NamedAiConfig {
                id: "default".to_string(),
                name: "default".to_string(),
                config: legacy,
            });
        }
    }

    // 先做全部只读校验并确定 id，再进可变作用域写入
    let name_dupe = |list: &[NamedAiConfig], exclude: Option<&str>| {
        list.iter()
            .any(|c| c.id != exclude.unwrap_or("") && c.name.eq_ignore_ascii_case(&name))
    };
    let id = match &input.id {
        Some(id) => {
            if !app.ai_configs.iter().any(|c| &c.id == id) {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    format!("AI 配置不存在: {id}"),
                ));
            }
            if name_dupe(&app.ai_configs, Some(id)) {
                return Err(Error::new(
                    ErrorCode::AiNotConfigured,
                    format!("配置名 {name:?} 已存在"),
                ));
            }
            id.clone()
        }
        None => {
            if name_dupe(&app.ai_configs, None) {
                return Err(Error::new(
                    ErrorCode::AiNotConfigured,
                    format!("配置名 {name:?} 已存在"),
                ));
            }
            new_id("cfg")
        }
    };
    let new_config = ProviderConfig {
        base_url,
        model,
        timeout_secs: input
            .timeout_secs
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .clamp(1, 600),
        max_tokens: input
            .max_tokens
            .unwrap_or(DEFAULT_MAX_TOKENS)
            .clamp(1, 32_768),
        provider: if input.provider.is_empty() {
            "openai-compatible".to_string()
        } else {
            input.provider
        },
        api_style: input.api_style,
        auth_method: input.auth_method,
        proxy_enabled: input.proxy_enabled,
        proxy_url: input
            .proxy_url
            .filter(|s| !s.trim().is_empty())
            .map(|s| s.trim().to_string()),
        context_window: input.context_window,
        max_retries: input
            .max_retries
            .unwrap_or(DEFAULT_MAX_RETRIES)
            .clamp(0, 10),
        cli_path,
        cli_args: input
            .cli_args
            .into_iter()
            .map(|a| a.trim().to_string())
            .filter(|a| !a.is_empty())
            .collect(),
        cli_env: input.cli_env,
    };
    if let Some(existing) = app.ai_configs.iter_mut().find(|c| c.id == id) {
        existing.name = name;
        existing.config = new_config;
    } else {
        app.ai_configs.push(NamedAiConfig {
            id: id.clone(),
            name,
            config: new_config,
        });
    }
    ensure_default(app);
    Ok(app
        .ai_configs
        .iter()
        .find(|c| c.id == id)
        .expect("config")
        .clone())
}

pub fn config_delete(app: &mut AppData, id: &str) -> Result<()> {
    if app.ai_configs.iter().any(|c| c.id == id) {
        app.ai_configs.retain(|c| c.id != id);
    } else {
        // 旧单配置视图：删除 default = 清空 ai
        if id == "default" {
            app.ai = None;
            return Ok(());
        }
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("AI 配置不存在: {id}"),
        ));
    }
    ensure_default(app);
    Ok(())
}

pub fn config_set_default(app: &mut AppData, id: &str) -> Result<()> {
    if configs(app).iter().any(|c| c.id == id) {
        app.ai_default_config = Some(id.to_string());
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::NotFound,
            format!("AI 配置不存在: {id}"),
        ))
    }
}

// ---------------------------------------------------------------------------
// 全局指令与模板
// ---------------------------------------------------------------------------

/// 保存全局自定义指令（trim；空串 = 清除；≤8000 字符）。
pub fn set_global_instructions(app: &mut AppData, text: &str) -> Result<String> {
    let trimmed = text.trim();
    if char_len(trimmed) > GLOBAL_INSTRUCTIONS_LIMIT {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            format!("全局指令超过 {GLOBAL_INSTRUCTIONS_LIMIT} 字符"),
        ));
    }
    let value = (!trimmed.is_empty()).then(|| trimmed.to_string());
    app.ai_global_instructions = value.clone();
    Ok(value.unwrap_or_default())
}

/// 保存（新建或更新）模板。限额：name ≤50、content ≤8000、激活总量 ≤16000（Unicode 字符）。
pub fn template_save(app: &mut AppData, input: TemplateSaveInput) -> Result<AiPromptTemplate> {
    let name = input.name.trim().to_string();
    if name.is_empty() || char_len(&name) > TEMPLATE_NAME_LIMIT {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "模板名不能为空且不超过 50 字符",
        ));
    }
    let content = input.content.trim().to_string();
    if content.is_empty() || char_len(&content) > TEMPLATE_CONTENT_LIMIT {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            format!("模板内容不能为空且不超过 {TEMPLATE_CONTENT_LIMIT} 字符"),
        ));
    }
    // 先只读校验 + 定 id，再进可变作用域写入
    let id = match &input.id {
        Some(id) => {
            if !app.ai_templates.iter().any(|t| t.id == *id) {
                return Err(Error::new(ErrorCode::NotFound, format!("模板不存在: {id}")));
            }
            if app
                .ai_templates
                .iter()
                .any(|t| t.id != *id && t.name.eq_ignore_ascii_case(&name))
            {
                return Err(Error::new(
                    ErrorCode::AiNotConfigured,
                    format!("模板名 {name:?} 已存在"),
                ));
            }
            id.clone()
        }
        None => {
            if app
                .ai_templates
                .iter()
                .any(|t| t.name.eq_ignore_ascii_case(&name))
            {
                return Err(Error::new(
                    ErrorCode::AiNotConfigured,
                    format!("模板名 {name:?} 已存在"),
                ));
            }
            new_id("tpl")
        }
    };
    // 激活总量先行校验（不落盘半写状态）：被更新模板按新内容计，未启用的不计
    let updating_enabled_id = input.id.as_deref().filter(|_| input.enabled);
    let current_active: usize = app
        .ai_templates
        .iter()
        .filter(|t| t.enabled && Some(t.id.as_str()) != updating_enabled_id)
        .map(|t| char_len(&t.content))
        .sum();
    if input.enabled && current_active + char_len(&content) > TEMPLATES_ACTIVE_TOTAL_LIMIT {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            format!(
                "启用模板总量超限：{} + {} > {TEMPLATES_ACTIVE_TOTAL_LIMIT} 字符",
                current_active,
                char_len(&content)
            ),
        ));
    }
    if let Some(t) = app.ai_templates.iter_mut().find(|t| t.id == id) {
        t.name = name;
        t.content = content;
        t.enabled = input.enabled;
    } else {
        app.ai_templates.push(AiPromptTemplate {
            id: id.clone(),
            name,
            content,
            enabled: input.enabled,
        });
    }
    Ok(app
        .ai_templates
        .iter()
        .find(|t| t.id == id)
        .expect("template")
        .clone())
}

pub fn template_delete(app: &mut AppData, id: &str) -> Result<()> {
    if app.ai_templates.iter().any(|t| t.id == id) {
        app.ai_templates.retain(|t| t.id != id);
        Ok(())
    } else {
        Err(Error::new(ErrorCode::NotFound, format!("模板不存在: {id}")))
    }
}

/// system 追加段：全局自定义指令 + 启用模板（按列表顺序）。
fn system_extras(app: &AppData) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(gi) = app
        .ai_global_instructions
        .as_deref()
        .filter(|s| !s.trim().is_empty())
    {
        parts.push(format!("用户全局指令（优先遵守）：\n{}", gi.trim()));
    }
    let templates: Vec<String> = app
        .ai_templates
        .iter()
        .filter(|t| t.enabled && !t.content.trim().is_empty())
        .map(|t| format!("[模板：{}]\n{}", t.name, t.content.trim()))
        .collect();
    if !templates.is_empty() {
        parts.push(format!("场景约定（模板）：\n{}", templates.join("\n\n")));
    }
    parts.join("\n\n")
}

// ---------------------------------------------------------------------------
// key 存取（应用级 secrets 文件；值绝不进入返回值/日志）
// ---------------------------------------------------------------------------

fn secret_file() -> PathBuf {
    crate::appdata::appdata_dir().join(AI_SECRET_FILE)
}

/// 读取 key；文件/条目缺失返回 Ok(None)。任何错误都不得把值带出去。
pub fn read_key() -> Result<Option<String>> {
    let path = secret_file();
    if !path.exists() {
        return Ok(None);
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::new(
            ErrorCode::SecretFileMissing,
            format!("无法读取 AI secrets 文件: {e}"),
        )
    })?;
    for (_, k, v) in crate::secrets::parse_dotenv(&text)? {
        if k == AI_SECRET_KEY {
            return Ok(Some(v));
        }
    }
    Ok(None)
}

/// 写入（原位替换）key。沿 secrets::set_key 的语义：临时文件 + 替换。
pub fn write_key(value: &str) -> Result<()> {
    if value.contains('\n') || value.contains('\r') {
        return Err(Error::new(ErrorCode::SpecInvalid, "AI key 只允许单行"));
    }
    let path = secret_file();
    let existing = std::fs::read_to_string(&path).unwrap_or_default();
    let mut lines: Vec<String> = if existing.is_empty() {
        Vec::new()
    } else {
        existing.lines().map(str::to_string).collect()
    };
    let replaced = lines
        .iter_mut()
        .find(|l| {
            l.trim()
                .split_once('=')
                .map(|(k, _)| k.trim() == AI_SECRET_KEY)
                .unwrap_or(false)
        })
        .is_some_and(|slot| {
            *slot = format!("{AI_SECRET_KEY}={value}");
            true
        });
    if !replaced {
        if !lines.is_empty() && !lines.last().is_some_and(|l| l.trim().is_empty()) {
            lines.push(String::new());
        }
        lines.push(format!("{AI_SECRET_KEY}={value}"));
    }
    let mut body = lines.join("\n");
    if !body.ends_with('\n') {
        body.push('\n');
    }
    atomic_write(&path, body.as_bytes())
}

/// 清除 key（只删该行；文件消失也视为成功）。
pub fn clear_key() -> Result<()> {
    let path = secret_file();
    if !path.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| {
        Error::new(
            ErrorCode::SecretFileMissing,
            format!("无法读取 AI secrets 文件: {e}"),
        )
    })?;
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        let is_target = line
            .trim()
            .split_once('=')
            .map(|(k, _)| k.trim() == AI_SECRET_KEY)
            .unwrap_or(false);
        if !is_target {
            out.push_str(line);
            out.push('\n');
        }
    }
    atomic_write(&path, out.as_bytes())
}

fn atomic_write(path: &PathBuf, bytes: &[u8]) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("无法创建目录: {e}")))?;
    }
    let tmp = path.with_extension("tmp-st");
    std::fs::write(&tmp, bytes)
        .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("写入临时文件失败: {e}")))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error::new(
            ErrorCode::TemplateWrite,
            format!("替换 secrets 文件失败: {e}"),
        )
    })?;
    Ok(())
}

// ---------------------------------------------------------------------------
// complete 编排
// ---------------------------------------------------------------------------

/// ai.complete 任务枚举（spec §5）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiTask {
    ExplainLogs,
    ConfigSuggest,
    EnrichDraft,
    TestConnection,
}

impl AiTask {
    pub fn parse(task: &str) -> Result<Self> {
        match task {
            "explain_logs" => Ok(Self::ExplainLogs),
            "config_suggest" => Ok(Self::ConfigSuggest),
            "enrich_draft" => Ok(Self::EnrichDraft),
            "test_connection" => Ok(Self::TestConnection),
            other => Err(Error::new(
                ErrorCode::Protocol,
                format!("未知 ai.complete task: {other}（允许 explain_logs | config_suggest | enrich_draft | test_connection）"),
            )),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplainLogs => "explain_logs",
            Self::ConfigSuggest => "config_suggest",
            Self::EnrichDraft => "enrich_draft",
            Self::TestConnection => "test_connection",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AiUsageOut {
    pub date: String,
    pub count: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct AiCompleteOut {
    pub text: String,
    pub usage: AiUsageOut,
    pub model: String,
    /// 本次响应的 token 用量（端点未返回时为 None）。
    pub tokens: Option<TokenUsage>,
}

/// ai.config.save 入参（id None = 新建）。
#[derive(Debug, Clone, Default)]
pub struct ConfigSaveInput {
    pub id: Option<String>,
    pub name: String,
    pub base_url: Option<String>,
    pub model: String,
    pub timeout_secs: Option<u64>,
    pub max_tokens: Option<u32>,
    pub provider: String,
    pub api_style: Option<ApiStyle>,
    pub auth_method: AuthMethod,
    pub proxy_enabled: bool,
    pub proxy_url: Option<String>,
    pub context_window: Option<u64>,
    pub max_retries: Option<u32>,
    pub cli_path: Option<String>,
    pub cli_args: Vec<String>,
    pub cli_env: BTreeMap<String, String>,
}

/// ai.template.save 入参（id None = 新建）。
#[derive(Debug, Clone)]
pub struct TemplateSaveInput {
    pub id: Option<String>,
    pub name: String,
    pub content: String,
    pub enabled: bool,
}

/// 一次 complete 的输入（key 由调用方注入以便测试；生产从 [`read_key`] 取）。
pub struct CompleteRequest<'a> {
    pub task: AiTask,
    pub payload: &'a serde_json::Value,
    /// 额外掩码值（workspace secret 值）；`supertask.ai` key 由本函数自行加入。
    pub extra_redact: &'a [String],
    /// 指定配置 id；None 用默认配置。
    pub config_id: Option<&'a str>,
}

/// 执行一次 AI 请求并记录当日用量（+1）。仅对 429/5xx 与瞬时网络错误自动重试；
/// **超时不再重试**（慢模型会重复计费且无法加速）。退避 500ms×尝试数。
/// `on_chunk` 非空时走 SSE 流式并在每块文本到达时回调（供壳层 `st-ai` 推送）。
pub fn complete<F: FnMut(&str)>(
    http: &dyn AiHttp,
    app: &mut AppData,
    key: Option<&str>,
    req: CompleteRequest<'_>,
    on_chunk: Option<F>,
) -> Result<AiCompleteOut> {
    complete_with(http, &cli_agent::ProcessCliRunner, app, key, req, on_chunk)
}

/// [`complete`] 的可注入版本：CLI provider 的进程执行器可替换，便于测试。
pub fn complete_with<F: FnMut(&str)>(
    http: &dyn AiHttp,
    cli: &dyn cli_agent::CliRunner,
    app: &mut AppData,
    key: Option<&str>,
    req: CompleteRequest<'_>,
    mut on_chunk: Option<F>,
) -> Result<AiCompleteOut> {
    let cfg = match req.config_id {
        Some(id) => configs(app)
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("AI 配置不存在: {id}")))?,
        None => default_config(app).ok_or_else(|| {
            Error::new(
                ErrorCode::AiNotConfigured,
                "AI 未配置，请先在 /ai 页新增配置",
            )
        })?,
    }
    .config;
    let style = cfg.effective_api_style();
    let key_optional = cfg.key_optional();
    let key = key
        .map(|k| k.to_string())
        .or_else(|| read_key().unwrap_or(None))
        .filter(|k| !k.is_empty());
    if key.is_none() && !key_optional {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "AI key 未设置，请先在 /ai 页保存密钥",
        ));
    }
    let key = key.unwrap_or_default();

    let mut redact: Vec<String> = req.extra_redact.to_vec();
    if !key.is_empty() {
        redact.push(key.clone());
    }

    let (base_system, user) = match req.task {
        AiTask::ExplainLogs => {
            let input = prompt::parse_input::<ExplainLogsInput>(req.payload)?;
            prompt::build_explain_logs(&input, &redact)
        }
        AiTask::ConfigSuggest => {
            let input = prompt::parse_input::<ConfigSuggestInput>(req.payload)?;
            prompt::build_config_suggest(&input, &redact)
        }
        AiTask::EnrichDraft => {
            let draft = req
                .payload
                .get("draft")
                .or(Some(req.payload))
                .and_then(|v| {
                    v.as_str()
                        .map(str::to_string)
                        .or_else(|| Some(v.to_string()))
                })
                .unwrap_or_default();
            prompt::build_enrich_draft(&draft, &redact)
        }
        AiTask::TestConnection => prompt::build_test_connection(),
    };
    let system = if req.task == AiTask::TestConnection {
        base_system
    } else {
        let extras = system_extras(app);
        if extras.is_empty() {
            base_system
        } else {
            format!("{base_system}\n\n{extras}")
        }
    };

    // 预算：字符 ÷4 粗估（spec §4.3）；context_window 更小时按窗口收口
    let budget = cfg
        .context_window
        .map(|w| w.min(MAX_CONTEXT_TOKENS as u64) as usize)
        .unwrap_or(MAX_CONTEXT_TOKENS);
    let est_tokens = (system.len() + user.len()) / 4;
    if est_tokens > budget {
        return Err(Error::new(
            ErrorCode::AiContextTooLarge,
            format!("上下文过大（约 {est_tokens} tokens > 上限 {budget}），请缩小日志范围"),
        ));
    }

    // 本地 CLI：没有 HTTP 请求可发，交给子进程执行器。
    if cfg.is_local_cli() {
        let text = run_local_cli(cli, &cfg, &system, &user)?;
        if let Some(ref mut chunk_cb) = on_chunk {
            // CLI 是一次性输出，没有真正的增量；一次性回调保持壳层流式接口不变。
            chunk_cb(&text);
        }
        return Ok(finish(app, text, None, cfg.model.clone()));
    }

    let stream = on_chunk.is_some();
    let max_tokens = match req.task {
        AiTask::TestConnection => cfg.max_tokens.min(16),
        _ => cfg.max_tokens,
    };
    let (suffix, body) =
        client::chat_request(style, &cfg.model, &system, &user, max_tokens, stream);
    let url = format!("{}{}", cfg.base_url.trim_end_matches('/'), suffix);
    let proxy = cfg.proxy_enabled.then(|| cfg.proxy_url.clone()).flatten();

    let mut last_err: Option<Error> = None;
    for attempt in 0..=cfg.max_retries {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_millis(500 * attempt as u64));
        }
        if let Some(ref mut chunk_cb) = on_chunk {
            match http.post_stream(
                &url,
                &key,
                cfg.auth_method,
                proxy.as_deref(),
                &body,
                cfg.timeout_secs,
                style,
                chunk_cb,
            ) {
                Ok((text, tokens)) => return Ok(finish(app, text, tokens, cfg.model.clone())),
                Err(e) => {
                    let transient = matches!(e.code(), ErrorCode::AiRequestFailed)
                        && (e.message().contains("返回 429")
                            || e.message().contains("返回 500")
                            || e.message().contains("返回 502")
                            || e.message().contains("返回 503")
                            || e.message().contains("返回 504"));
                    if !transient {
                        return Err(e);
                    }
                    last_err = Some(e);
                }
            }
        } else {
            match http.post(
                &url,
                &key,
                cfg.auth_method,
                proxy.as_deref(),
                &body,
                cfg.timeout_secs,
            ) {
                Ok(resp) => {
                    if (200..300).contains(&resp.status) {
                        let (text, tokens) = parse_chat_response(style, &resp.body)?;
                        return Ok(finish(app, text, tokens, cfg.model.clone()));
                    }
                    // 临时错误重试；其余立即失败
                    let transient = matches!(resp.status, 429 | 500 | 502 | 503 | 504);
                    let snippet: String = resp.body.chars().take(300).collect();
                    let err = Error::new(
                        ErrorCode::AiRequestFailed,
                        format!(
                            "AI 端点返回 {}: {}",
                            resp.status,
                            redact_key(&snippet, &key)
                        ),
                    );
                    if !transient {
                        return Err(err);
                    }
                    last_err = Some(err);
                }
                Err(e) => {
                    // 超时 = 模型/链路仍在生成，重试只会重复打满额请求；仅网络瞬时失败可重试
                    let transient = matches!(e.code(), ErrorCode::AiRequestFailed);
                    if !transient {
                        return Err(e);
                    }
                    last_err = Some(e);
                }
            }
        }
    }
    Err(last_err.expect("retry loop ran at least once"))
}

/// OpenAI 兼容模型发现：`GET {base}/models`（anthropic 风格不支持，报 `AI_REQUEST_FAILED`）。
pub fn models(
    http: &dyn AiHttp,
    app: &AppData,
    key: Option<&str>,
    config_id: Option<&str>,
) -> Result<Vec<String>> {
    let cfg = match config_id {
        Some(id) => configs(app)
            .into_iter()
            .find(|c| c.id == id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("AI 配置不存在: {id}")))?,
        None => default_config(app).ok_or_else(|| {
            Error::new(
                ErrorCode::AiNotConfigured,
                "AI 未配置，请先在 /ai 页新增配置",
            )
        })?,
    }
    .config;
    // CLI provider 没有 /models 端点；给预置项而不是报错，用户仍可手填任意模型名
    if cfg.is_local_cli() {
        return Ok(provider_preset(&cfg.provider)
            .map(|p| p.cli_models.iter().map(|m| m.to_string()).collect())
            .unwrap_or_default());
    }
    if cfg.effective_api_style() != ApiStyle::OpenAiCompletions {
        return Err(Error::new(
            ErrorCode::AiRequestFailed,
            "当前 API 风格不支持模型发现，请手动填写模型名",
        ));
    }
    let key = key
        .map(|k| k.to_string())
        .or_else(|| read_key().unwrap_or(None))
        .filter(|k| !k.is_empty())
        .unwrap_or_default();
    let url = format!("{}/models", cfg.base_url.trim_end_matches('/'));
    let proxy = cfg.proxy_enabled.then(|| cfg.proxy_url.clone()).flatten();
    let resp = http.get(
        &url,
        &key,
        cfg.auth_method,
        proxy.as_deref(),
        cfg.timeout_secs,
    )?;
    if !(200..300).contains(&resp.status) {
        let snippet: String = resp.body.chars().take(200).collect();
        return Err(Error::new(
            ErrorCode::AiRequestFailed,
            format!(
                "模型列表返回 {}: {}",
                resp.status,
                redact_key(&snippet, &key)
            ),
        ));
    }
    parse_models_response(&resp.body)
}

/// 跑一次本地 CLI：prompt 走 stdin（不进 argv，避免命令行长度上限与转义问题）。
fn run_local_cli(
    cli: &dyn cli_agent::CliRunner,
    cfg: &ProviderConfig,
    system: &str,
    user: &str,
) -> Result<String> {
    let program = cfg.cli_program();
    if program.is_empty() {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "未配置 CLI 可执行文件路径",
        ));
    }
    let mut args = cfg.effective_cli_args();
    let model = cfg.model.trim();
    // "default" = 用 CLI 自己的默认模型，不传 --model
    if !model.is_empty() && !model.eq_ignore_ascii_case("default") {
        args.push("--model".to_string());
        args.push(model.to_string());
    }
    let invocation = cli_agent::CliInvocation {
        program: program.clone(),
        args,
        env: cli_agent::build_env(&cfg.cli_env)?,
        stdin: format!("{system}\n\n{user}"),
        timeout_secs: cfg.timeout_secs,
    };
    let out = cli.run(&invocation)?;
    if !out.success() {
        return Err(cli_agent::run_error(&program, &out));
    }
    let text = cli_agent::extract_text(&out.stdout);
    if text.is_empty() {
        return Err(Error::new(
            ErrorCode::AiRequestFailed,
            format!("{program} 没有返回任何文本输出"),
        ));
    }
    Ok(text)
}

fn finish(
    app: &mut AppData,
    text: String,
    tokens: Option<TokenUsage>,
    model: String,
) -> AiCompleteOut {
    let date = today_utc();
    let usage = match &mut app.ai_usage {
        Some(u) if u.date == date => {
            u.count += 1;
            u.clone()
        }
        _ => {
            let u = AiUsage {
                date: date.clone(),
                count: 1,
            };
            app.ai_usage = Some(u.clone());
            u
        }
    };
    AiCompleteOut {
        text,
        usage: AiUsageOut {
            date: usage.date,
            count: usage.count,
        },
        model,
        tokens,
    }
}

/// 当前 UTC 日期（YYYY-MM-DD）；零第三方依赖（civil-from-days）。
pub fn today_utc() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() / 86_400)
        .unwrap_or(0) as i64;
    // Howard Hinnant civil_from_days
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::sync::Mutex;

    // -------------------------------------------------------------------
    // fake 传输（沿 FakeRunner 先例：矩阵单测零网络）
    // -------------------------------------------------------------------

    struct FakeHttp {
        responses: Mutex<Vec<Result<AiHttpResponse>>>,
        calls: Mutex<Vec<(String, String, Option<String>)>>, // (url, key, proxy)
    }
    impl FakeHttp {
        fn ok(status: u16, body: &str, times: usize) -> Self {
            Self {
                responses: Mutex::new(
                    (0..times)
                        .map(|_| {
                            Ok(AiHttpResponse {
                                status,
                                body: body.to_string(),
                            })
                        })
                        .collect(),
                ),
                calls: Mutex::new(Vec::new()),
            }
        }
        fn ok_json(times: usize) -> Self {
            Self::ok(
                200,
                &serde_json::json!({
                    "choices": [{ "message": { "content": "建议：把端口改为 8081" } }],
                    "usage": { "prompt_tokens": 42, "completion_tokens": 7 }
                })
                .to_string(),
                times,
            )
        }
    }
    impl AiHttp for FakeHttp {
        fn post(
            &self,
            url: &str,
            api_key: &str,
            _auth: AuthMethod,
            proxy_url: Option<&str>,
            _body: &str,
            _timeout_secs: u64,
        ) -> Result<AiHttpResponse> {
            self.calls.lock().unwrap().push((
                url.to_string(),
                api_key.to_string(),
                proxy_url.map(str::to_string),
            ));
            self.responses.lock().unwrap().remove(0)
        }
    }

    fn named(id: &str, name: &str, patch: impl FnOnce(&mut ProviderConfig)) -> NamedAiConfig {
        let mut config = ProviderConfig {
            base_url: "http://localhost:9999/v1".into(),
            model: "demo-model".into(),
            timeout_secs: 5,
            max_tokens: 128,
            ..ProviderConfig::default()
        };
        patch(&mut config);
        NamedAiConfig {
            id: id.into(),
            name: name.into(),
            config,
        }
    }

    fn app_multi() -> AppData {
        let mut app = AppData::default();
        app.ai_configs = vec![
            named("cfg-a", "A", |_| {}),
            named("cfg-b", "B", |c| {
                c.proxy_enabled = true;
                c.proxy_url = Some("127.0.0.1:7890".into());
            }),
        ];
        app.ai_default_config = Some("cfg-a".into());
        app
    }

    fn payload(task: AiTask) -> serde_json::Value {
        match task {
            AiTask::ExplainLogs => {
                serde_json::json!({ "lines": ["ERROR o.s.b.PropertySourceLoader: invalid"], "service": { "id": "api", "kind": "spring-boot", "port": 8080 } })
            }
            AiTask::ConfigSuggest => {
                serde_json::json!({ "yaml": "services:\n  api:\n    port: 8080\n", "problems": ["端口被占用"] })
            }
            AiTask::EnrichDraft => serde_json::json!({ "draft": { "items": [] } }),
            AiTask::TestConnection => serde_json::json!({}),
        }
    }

    fn run(http: &FakeHttp, app: &mut AppData, task: AiTask) -> Result<AiCompleteOut> {
        complete(
            http,
            app,
            Some("sk-test-key-123456"),
            CompleteRequest {
                task,
                payload: &payload(task),
                extra_redact: &[],
                config_id: None,
            },
            None::<fn(&str)>,
        )
    }

    #[test]
    fn base_url_validation_matrix() {
        let ok = [
            "http://localhost:11434/v1",
            "https://api.example.com/v1/",
            "http://10.0.0.2:8000",
        ];
        for (i, u) in ok.iter().enumerate() {
            assert_eq!(
                validate_base_url(u).unwrap(),
                u.trim_end_matches('/'),
                "#{i}"
            );
        }
        let bad = [
            "",
            "localhost:11434",
            "ftp://x",
            "http://",
            "http://u:p@host/v1",
            "http://host/v1?x=1",
            "http://host:port",
            "http://host/v1#f",
            "http://host :80/v1",
        ];
        for u in bad {
            assert!(validate_base_url(u).is_err(), "应拒绝 {u:?}");
        }
    }

    #[test]
    fn presets_cover_expected_providers() {
        assert!(provider_preset("openai-compatible").is_some());
        assert!(provider_preset("claude").is_some());
        assert!(provider_preset("ollama").is_some_and(|p| p.key_optional));
        assert!(provider_preset("openai-compatible").is_some_and(|p| !p.key_optional));
        assert!(provider_preset("codex-cli").is_some_and(|p| {
            p.kind == ProviderKind::LocalCli && p.cli_program == "codex" && p.key_optional
        }));
        assert!(provider_preset("claude-code-cli").is_some_and(|p| !p.cli_args.is_empty()));
        assert!(provider_preset("custom").is_some_and(|p| p.default_endpoint.is_empty()));
    }

    #[test]
    fn named_config_crud_and_default_semantics() {
        let mut app = AppData::default();
        // 旧单配置迁移视图
        app.ai = Some(ProviderConfig {
            base_url: "http://legacy/v1".into(),
            model: "m".into(),
            ..Default::default()
        });
        assert_eq!(configs(&app).len(), 1);
        assert_eq!(configs(&app)[0].id, "default");

        // 首次保存触发迁移；迁移来的 legacy 配置保持默认（此前生效的配置不变）
        let saved = config_save(
            &mut app,
            ConfigSaveInput {
                name: "A".into(),
                base_url: Some("http://a/v1".into()),
                model: "ma".into(),
                provider: "deepseek".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(app.ai.is_none(), "迁移后清除旧字段");
        assert_eq!(
            app.ai_default_config.as_deref(),
            Some("default"),
            "迁移配置保持默认"
        );
        // 重名拒绝（大小写不敏感）
        assert!(config_save(
            &mut app,
            ConfigSaveInput {
                name: "a".into(),
                base_url: Some("http://b/v1".into()),
                model: "m".into(),
                ..Default::default()
            }
        )
        .is_err());

        let saved2 = config_save(
            &mut app,
            ConfigSaveInput {
                name: "B".into(),
                base_url: Some("http://b/v1".into()),
                model: "mb".into(),
                ..Default::default()
            },
        )
        .unwrap();
        assert_ne!(
            app.ai_default_config.as_deref(),
            Some(saved2.id.as_str()),
            "后加者不抢默认"
        );
        config_set_default(&mut app, &saved2.id).unwrap();
        assert_eq!(default_config(&app).unwrap().name, "B");
        config_delete(&mut app, &saved2.id).unwrap();
        assert_eq!(
            default_config(&app).unwrap().id,
            "default",
            "删除默认后回退首个"
        );
        assert!(config_delete(&mut app, "missing").is_err());
        let _ = saved;
    }

    #[test]
    fn config_save_validates_proxy_and_provider() {
        let mut app = AppData::default();
        let e = config_save(
            &mut app,
            ConfigSaveInput {
                name: "P".into(),
                base_url: Some("http://x/v1".into()),
                model: "m".into(),
                proxy_enabled: true,
                proxy_url: Some("ftp://bad".into()),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiNotConfigured);
        let e = config_save(
            &mut app,
            ConfigSaveInput {
                name: "P".into(),
                base_url: Some("http://x/v1".into()),
                model: "m".into(),
                provider: "unknown-provider".into(),
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiNotConfigured);

        let cli = config_save(
            &mut app,
            ConfigSaveInput {
                name: "CLI".into(),
                base_url: None,
                model: "default".into(),
                provider: "codex-cli".into(),
                cli_path: Some("codex".into()),
                ..Default::default()
            },
        )
        .unwrap();
        assert!(cli.config.is_local_cli());
        assert!(cli.config.base_url.is_empty());
    }

    #[test]
    fn complete_uses_default_config_and_proxy_passes_through() {
        let http = FakeHttp::ok_json(1);
        let mut app = app_multi();
        let out = run(&http, &mut app, AiTask::ExplainLogs).unwrap();
        assert_eq!(out.model, "demo-model");
        let (url, key, proxy) = http.calls.lock().unwrap().last().unwrap().clone();
        assert!(url.starts_with("http://localhost:9999/v1/chat/completions"));
        assert_eq!(key, "sk-test-key-123456");
        assert_eq!(proxy, None, "默认配置未开代理");
        assert_eq!(app.ai_usage.as_ref().unwrap().count, 1);

        // 指定带代理的配置
        let http2 = FakeHttp::ok_json(1);
        let out2 = complete(
            &http2,
            &mut app,
            Some("k"),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &payload(AiTask::ExplainLogs),
                extra_redact: &[],
                config_id: Some("cfg-b"),
            },
            None::<fn(&str)>,
        )
        .unwrap();
        assert_eq!(out2.model, "demo-model");
        let (_, _, proxy2) = http2.calls.lock().unwrap().last().unwrap().clone();
        assert_eq!(proxy2.as_deref(), Some("127.0.0.1:7890"));
    }

    #[test]
    fn complete_ollama_allows_missing_key() {
        let mut app = AppData::default();
        app.ai_configs = vec![named("o", "local", |c| {
            c.provider = "ollama".into();
        })];
        let http = FakeHttp::ok_json(1);
        // 传 Some("") 而非 None：None 会回退读真实 secrets 文件（真机已配 key 时污染测试）
        let out = complete(
            &http,
            &mut app,
            Some(""),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &payload(AiTask::ExplainLogs),
                extra_redact: &[],
                config_id: None,
            },
            None::<fn(&str)>,
        )
        .unwrap();
        assert_eq!(out.model, "demo-model");
        let (_, key, _) = http.calls.lock().unwrap().last().unwrap().clone();
        assert_eq!(key, "", "ollama 免鉴权：空 key 也发出请求");

        // 非 ollama 缺 key → AI_NOT_CONFIGURED
        let mut app2 = app_multi();
        let e = complete(
            &http,
            &mut app2,
            Some(""),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &payload(AiTask::ExplainLogs),
                extra_redact: &[],
                config_id: None,
            },
            None::<fn(&str)>,
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiNotConfigured);
    }

    #[test]
    fn complete_retries_transient_then_succeeds() {
        // 1 次 429 + 1 次成功（按调用顺序消费）
        let responses: Vec<Result<AiHttpResponse>> = vec![
            Ok(AiHttpResponse {
                status: 429,
                body: "rate limited".into(),
            }),
            Ok(AiHttpResponse {
                status: 200,
                body: serde_json::json!({ "choices": [{ "message": { "content": "ok" } }] })
                    .to_string(),
            }),
        ];
        let http = FakeHttp {
            responses: Mutex::new(responses),
            calls: Mutex::new(Vec::new()),
        };
        let mut app = app_multi();
        let out = run(&http, &mut app, AiTask::ExplainLogs).unwrap();
        assert_eq!(out.text, "ok");
        assert_eq!(http.calls.lock().unwrap().len(), 2, "重试了一次");
        assert_eq!(
            app.ai_usage.as_ref().unwrap().count,
            1,
            "一次调用只计一次用量"
        );
    }

    #[test]
    fn complete_non_transient_4xx_fails_immediately() {
        let http = FakeHttp::ok(401, "invalid key sk-test-key-123456", 1);
        let mut app = app_multi();
        let e = run(&http, &mut app, AiTask::ConfigSuggest).unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiRequestFailed);
        assert!(
            !e.message().contains("sk-test-key-123456"),
            "错误信息不回显 key"
        );
        assert!(e.message().contains("<redacted>"));
        assert_eq!(http.calls.lock().unwrap().len(), 1, "非临时错误不重试");
        assert!(app.ai_usage.is_none(), "失败调用不计用量");
    }

    #[test]
    fn complete_timeout_does_not_retry() {
        let http = FakeHttp {
            responses: Mutex::new(vec![Err(Error::new(
                ErrorCode::AiTimeout,
                "AI 请求超时（超过 timeout_secs）",
            ))]),
            calls: Mutex::new(Vec::new()),
        };
        let mut app = app_multi();
        let e = run(&http, &mut app, AiTask::ExplainLogs).unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiTimeout);
        assert_eq!(
            http.calls.lock().unwrap().len(),
            1,
            "超时不应重试，避免慢响应重复计费"
        );
        assert!(app.ai_usage.is_none(), "失败调用不计用量");
    }

    #[test]
    fn complete_exhausts_retries_then_last_error() {
        let http = FakeHttp::ok(503, "down", 3);
        let mut app = app_multi();
        let e = run(&http, &mut app, AiTask::ExplainLogs).unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiRequestFailed);
        assert_eq!(
            http.calls.lock().unwrap().len(),
            3,
            "默认 max_retries=2 → 共 3 次"
        );
    }

    #[test]
    fn complete_budget_uses_context_window_floor() {
        let mut app = app_multi();
        app.ai_configs[0].config.context_window = Some(1_000);
        let http = FakeHttp::ok_json(1);
        let big = "x".repeat(6_000);
        let e = complete(
            &http,
            &mut app,
            Some("k"),
            CompleteRequest {
                task: AiTask::ConfigSuggest,
                payload: &serde_json::json!({ "yaml": big, "problems": [] }),
                extra_redact: &[],
                config_id: None,
            },
            None::<fn(&str)>,
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiContextTooLarge);
    }

    #[test]
    fn complete_unknown_task_and_bad_payload() {
        let http = FakeHttp::ok_json(1);
        let mut app = app_multi();
        let e = complete(
            &http,
            &mut app,
            Some("k"),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &serde_json::json!({ "lines": 1 }),
                extra_redact: &[],
                config_id: None,
            },
            None::<fn(&str)>,
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::Protocol);
        assert!(AiTask::parse("auto").is_err());
        assert_eq!(
            AiTask::parse("config_suggest").unwrap().as_str(),
            "config_suggest"
        );
        let e = complete(
            &http,
            &mut app,
            Some("k"),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &payload(AiTask::ExplainLogs),
                extra_redact: &[],
                config_id: Some("nope"),
            },
            None::<fn(&str)>,
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::NotFound);
    }

    #[test]
    fn complete_masks_secret_values_in_prompt_body() {
        struct CaptureBody(Mutex<Vec<String>>);
        impl AiHttp for CaptureBody {
            fn post(
                &self,
                _u: &str,
                _k: &str,
                _a: AuthMethod,
                _p: Option<&str>,
                body: &str,
                _t: u64,
            ) -> Result<AiHttpResponse> {
                self.0.lock().unwrap().push(body.to_string());
                Ok(AiHttpResponse {
                    status: 200,
                    body: serde_json::json!({ "choices": [{ "message": { "content": "ok" } }] })
                        .to_string(),
                })
            }
        }
        let http = CaptureBody(Mutex::new(Vec::new()));
        let mut app = app_multi();
        complete(
            &http,
            &mut app,
            Some("sk-test-key-123456"),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &serde_json::json!({
                    "lines": ["connect failed: password=hunter2secret"],
                    "service": { "id": "api", "kind": "spring-boot" }
                }),
                extra_redact: &["hunter2secret".to_string()],
                config_id: None,
            },
            None::<fn(&str)>,
        )
        .unwrap();
        let body = http.0.lock().unwrap().last().unwrap().clone();
        assert!(
            !body.contains("hunter2secret"),
            "原始 secret 值不得出现在请求体"
        );
        assert!(!body.contains("sk-test-key-123456"), "key 不得出现在请求体");
        assert!(body.contains(REDACTED));
    }

    #[test]
    fn global_instructions_and_templates_inject_into_system() {
        let mut app = app_multi();
        set_global_instructions(&mut app, "始终用中文回答；端口建议避开 8080。").unwrap();
        template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "排错风格".into(),
                content: "按「现象 → 根因假设 → 验证步骤」三段输出。".into(),
                enabled: true,
            },
        )
        .unwrap();
        let disabled = template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "未启用".into(),
                content: "SHOULD_NOT_APPEAR".into(),
                enabled: false,
            },
        )
        .unwrap();
        let _ = disabled;
        let http = CaptureBodySys::new();
        complete(
            &http,
            &mut app,
            Some("k"),
            CompleteRequest {
                task: AiTask::ExplainLogs,
                payload: &payload(AiTask::ExplainLogs),
                extra_redact: &[],
                config_id: None,
            },
            None::<fn(&str)>,
        )
        .unwrap();
        let body = http.last();
        assert!(body.contains("始终用中文回答"));
        assert!(body.contains("排错风格"));
        assert!(!body.contains("SHOULD_NOT_APPEAR"), "未启用模板不注入");
    }

    struct CaptureBodySys(Mutex<Vec<String>>);
    impl CaptureBodySys {
        fn new() -> Self {
            Self(Mutex::new(Vec::new()))
        }
        fn last(&self) -> String {
            self.0.lock().unwrap().last().unwrap().clone()
        }
    }
    impl AiHttp for CaptureBodySys {
        fn post(
            &self,
            _u: &str,
            _k: &str,
            _a: AuthMethod,
            _p: Option<&str>,
            body: &str,
            _t: u64,
        ) -> Result<AiHttpResponse> {
            self.0.lock().unwrap().push(body.to_string());
            Ok(AiHttpResponse {
                status: 200,
                body: serde_json::json!({ "choices": [{ "message": { "content": "ok" } }] })
                    .to_string(),
            })
        }
    }

    #[test]
    fn template_limits_enforced() {
        let mut app = AppData::default();
        assert!(set_global_instructions(&mut app, &"x".repeat(8_001)).is_err());
        assert_eq!(set_global_instructions(&mut app, "  ok  ").unwrap(), "ok");
        assert!(template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "".into(),
                content: "c".into(),
                enabled: true
            }
        )
        .is_err());
        // 单模板 ≤8000 字符
        assert!(template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "t".into(),
                content: "x".repeat(8_001),
                enabled: true
            }
        )
        .is_err());
        template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "t1".into(),
                content: "x".repeat(8_000),
                enabled: true,
            },
        )
        .unwrap();
        // 激活总量 16000：再启用一个 8000 恰好达线，第三个超限
        template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "t2".into(),
                content: "y".repeat(8_000),
                enabled: true,
            },
        )
        .unwrap();
        let e = template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "t3".into(),
                content: "z".repeat(8_000),
                enabled: true,
            },
        )
        .unwrap_err();
        assert!(e.message().contains("16"));
        // 未启用不受总量约束
        template_save(
            &mut app,
            TemplateSaveInput {
                id: None,
                name: "t4".into(),
                content: "w".repeat(8_000),
                enabled: false,
            },
        )
        .unwrap();
        template_delete(&mut app, "missing").unwrap_err();
    }

    #[test]
    fn usage_resets_next_day() {
        let mut app = AppData::default();
        app.ai_usage = Some(AiUsage {
            date: "2000-01-01".into(),
            count: 42,
        });
        assert_eq!(
            app.ai_usage.as_ref().unwrap().today_count(),
            0,
            "非当日归零展示"
        );
        let today = today_utc();
        app.ai_usage = Some(AiUsage {
            date: today.clone(),
            count: 7,
        });
        assert_eq!(app.ai_usage.as_ref().unwrap().today_count(), 7);
        assert_eq!(today.len(), 10);
        assert_eq!(&today[4..5], "-");
    }

    #[test]
    fn appdata_roundtrip_with_ai_fields() {
        let dir = std::env::temp_dir().join(format!("st-ai-appdata-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("app.json");
        let mut app = AppData::default();
        app.ai_configs = vec![named("c1", "Local", |c| {
            c.provider = "ollama".into();
            c.context_window = Some(8192);
            c.max_retries = 3;
        })];
        app.ai_default_config = Some("c1".into());
        app.ai_global_instructions = Some("用中文".into());
        app.ai_templates = vec![AiPromptTemplate {
            id: "t1".into(),
            name: "T".into(),
            content: "C".into(),
            enabled: true,
        }];
        app.ai_usage = Some(AiUsage {
            date: today_utc(),
            count: 3,
        });
        crate::appdata::save_at(&path, &app).unwrap();
        let loaded = crate::appdata::load_at(&path);
        assert_eq!(loaded, app);
        let disk = std::fs::read_to_string(&path).unwrap();
        assert!(disk.contains("aiConfigs"), "camelCase 字段名");
        assert!(disk.contains("contextWindow"));
        assert!(disk.contains("maxRetries"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn key_store_roundtrip_and_clear_isolated_to_appdata() {
        // 直接写文件路径而非全局 APPDATA：用临时目录验证读写逻辑等价物。
        let dir = std::env::temp_dir().join(format!("st-ai-key-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(AI_SECRET_FILE);
        atomic_write(
            &path,
            format!("{AI_SECRET_KEY}=sk-abc\nOTHER=1\n").as_bytes(),
        )
        .unwrap();
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(&format!("{AI_SECRET_KEY}=sk-abc\n")));
        let entries = crate::secrets::parse_dotenv(&text).unwrap();
        assert_eq!(entries[0], (1, AI_SECRET_KEY.into(), "sk-abc".into()));
        assert_eq!(entries[1], (2, "OTHER".into(), "1".into()));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
