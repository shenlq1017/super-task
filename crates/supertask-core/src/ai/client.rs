//! 2.1 §4.1 AiHttp 传输抽象 + OpenAI 兼容 / Anthropic Messages 响应解析。
//!
//! 复用 v2.0 的 ureq + rustls（复用核查见 v2.1 实现计划）；执行抽象沿
//! `cloud/http.rs` 的 HttpExecutor 先例，错误映射矩阵用本地 fake 单测，零真实网络。
//! 超时、代理与错误脱敏策略参考 dbx（references/dbx/2026-08-29-8f54385/）：
//! - `build_ai_http_client` 的 per-request timeout → [`AiHttp::post`] 的 `timeout_secs`；
//! - 代理：裸 `host:port` 自动补 `http://`；loopback 端点强制绕过代理（`build_ai_http_client`）；
//! - `categorized_http_error` 的「错误文本不得回显 key」→ [`redact_key`]。

use std::error::Error as _;
use std::io::{BufRead, BufReader};
use std::time::Duration;

use crate::error::{Error, ErrorCode, Result};
use serde::{Deserialize, Serialize};

use super::{ApiStyle, AuthMethod};

/// OpenAI 兼容响应中可选的 token 用量（Anthropic 映射 input/output_tokens）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct TokenUsage {
    #[serde(default)]
    pub prompt_tokens: u64,
    #[serde(default)]
    pub completion_tokens: u64,
}

/// `POST <chat endpoint>` 的原始结果（status + body）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AiHttpResponse {
    pub status: u16,
    pub body: String,
}

/// HTTP 执行抽象：URL / key / 请求体 / 超时 / 代理 → 原始响应；传输失败返回 Err。
pub trait AiHttp: Send + Sync {
    fn post(
        &self,
        url: &str,
        api_key: &str,
        auth: AuthMethod,
        proxy_url: Option<&str>,
        body: &str,
        timeout_secs: u64,
    ) -> Result<AiHttpResponse>;

    /// 模型发现等 GET 场景（`ai.models`）。
    fn get(
        &self,
        url: &str,
        api_key: &str,
        auth: AuthMethod,
        proxy_url: Option<&str>,
        timeout_secs: u64,
    ) -> Result<AiHttpResponse> {
        let _ = (url, api_key, auth, proxy_url, timeout_secs);
        Err(Error::new(
            ErrorCode::AiRequestFailed,
            "该传输实现不支持 GET",
        ))
    }

    /// 流式 chat：逐块回调 delta，返回完整文本与可选 token 用量。
    /// 默认实现退化为阻塞 `post` 后一次性回调（fake 注入与单测沿用）。
    fn post_stream(
        &self,
        url: &str,
        api_key: &str,
        auth: AuthMethod,
        proxy_url: Option<&str>,
        body: &str,
        timeout_secs: u64,
        style: ApiStyle,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<(String, Option<TokenUsage>)> {
        let resp = self.post(url, api_key, auth, proxy_url, body, timeout_secs)?;
        if !(200..300).contains(&resp.status) {
            let snippet: String = resp.body.chars().take(300).collect();
            return Err(Error::new(
                ErrorCode::AiRequestFailed,
                format!(
                    "AI 端点返回 {}: {}",
                    resp.status,
                    redact_key(&snippet, api_key)
                ),
            ));
        }
        let (text, usage) = parse_chat_response(style, &resp.body)?;
        if !text.is_empty() {
            on_delta(&text);
        }
        Ok((text, usage))
    }
}

fn is_loopback_url(url: &str) -> bool {
    let after_scheme = url.split("://").nth(1).unwrap_or(url);
    let host = after_scheme
        .split(['/', '?', '#'])
        .next()
        .unwrap_or("")
        .rsplit_once(':')
        .map(|(h, _)| h)
        .unwrap_or(after_scheme);
    let host = host.trim_matches(['[', ']']);
    host == "localhost" || host == "127.0.0.1" || host == "::1" || host.ends_with(".localhost")
}

/// 裸 `host:port` 自动补 `http://`（dbx `build_ai_http_client` 策略）。
pub fn normalize_proxy_url(proxy_url: &str) -> Result<String> {
    let trimmed = proxy_url.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "代理地址不能为空或含空白",
        ));
    }
    if trimmed.contains("://") {
        let scheme = trimmed.split("://").next().unwrap_or("").to_lowercase();
        if scheme != "http" && scheme != "https" && scheme != "socks5" {
            return Err(Error::new(
                ErrorCode::AiNotConfigured,
                "代理只允许 http/https/socks5",
            ));
        }
        Ok(trimmed.to_string())
    } else {
        Ok(format!("http://{trimmed}"))
    }
}

/// 组装请求头并发送。生产实现为阻塞式 ureq（json + rustls）。
/// 超时错误归一 `AI_TIMEOUT`，其余传输错误归一 `AI_REQUEST_FAILED`；key 只进 header。
fn request_with_ureq(
    method: &str,
    url: &str,
    api_key: &str,
    auth: AuthMethod,
    proxy_url: Option<&str>,
    body: Option<&str>,
    timeout_secs: u64,
    extra_headers: &[(&str, &str)],
) -> Result<AiHttpResponse> {
    let proxy =
        match proxy_url.filter(|_| !is_loopback_url(url)) {
            Some(p) => Some(ureq::Proxy::new(normalize_proxy_url(p)?).map_err(|e| {
                Error::new(ErrorCode::AiNotConfigured, format!("代理地址无效: {e}"))
            })?),
            None => None,
        };
    let agent = {
        let mut ab = ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout_secs.max(1)));
        if let Some(p) = proxy {
            ab = ab.proxy(p);
        }
        ab.build()
    };
    let mut builder = agent.request(method, url);
    if !api_key.is_empty() {
        builder = match auth {
            AuthMethod::ApiKey if url_is_anthropic(url) => builder.set("x-api-key", api_key),
            _ => builder.set("Authorization", &format!("Bearer {api_key}")),
        };
    }
    for (k, v) in extra_headers {
        builder = builder.set(k, v);
    }
    builder = builder.set("Content-Type", "application/json");
    let resp = match body {
        Some(b) => builder.send_string(b),
        None => builder.call(),
    };
    match resp {
        Ok(r) => {
            let status = r.status();
            let body = r.into_string().unwrap_or_default();
            Ok(AiHttpResponse { status, body })
        }
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            Ok(AiHttpResponse { status: code, body })
        }
        Err(e) => {
            if is_timeout_error(&e) {
                Err(Error::new(
                    ErrorCode::AiTimeout,
                    "AI 请求超时（超过 timeout_secs）",
                ))
            } else {
                Err(Error::new(
                    ErrorCode::AiRequestFailed,
                    format!("AI 端点不可达: {}", transport_message(&e)),
                ))
            }
        }
    }
}

fn url_is_anthropic(url: &str) -> bool {
    url.contains("/v1/messages")
}

fn is_timeout_error(err: &ureq::Error) -> bool {
    let ureq::Error::Transport(t) = err else {
        return false;
    };
    t.source().is_some_and(|s| {
        s.downcast_ref::<std::io::Error>()
            .is_some_and(|io| io.kind() == std::io::ErrorKind::TimedOut)
    }) || t.to_string().contains("timed out")
}

fn transport_message(e: &ureq::Error) -> String {
    let full = e.to_string();
    // URL 可能含敏感 query（理论上不该有，防御性剥离）
    match full.split_once('?') {
        Some((prefix, _)) => format!("{prefix}?<redacted>"),
        None => full,
    }
}

/// 生产实现。
pub struct UreqAiHttp;

impl AiHttp for UreqAiHttp {
    fn post(
        &self,
        url: &str,
        api_key: &str,
        auth: AuthMethod,
        proxy_url: Option<&str>,
        body: &str,
        timeout_secs: u64,
    ) -> Result<AiHttpResponse> {
        let extra: &[(&str, &str)] = if url.contains("/v1/messages") {
            &[("anthropic-version", "2023-06-01")]
        } else {
            &[]
        };
        request_with_ureq(
            "POST",
            url,
            api_key,
            auth,
            proxy_url,
            Some(body),
            timeout_secs,
            extra,
        )
    }

    fn post_stream(
        &self,
        url: &str,
        api_key: &str,
        auth: AuthMethod,
        proxy_url: Option<&str>,
        body: &str,
        timeout_secs: u64,
        style: ApiStyle,
        on_delta: &mut dyn FnMut(&str),
    ) -> Result<(String, Option<TokenUsage>)> {
        let extra: &[(&str, &str)] = if url.contains("/v1/messages") {
            &[("anthropic-version", "2023-06-01")]
        } else {
            &[]
        };
        request_stream_with_ureq(
            url,
            api_key,
            auth,
            proxy_url,
            body,
            timeout_secs,
            extra,
            style,
            on_delta,
        )
    }

    fn get(
        &self,
        url: &str,
        api_key: &str,
        auth: AuthMethod,
        proxy_url: Option<&str>,
        timeout_secs: u64,
    ) -> Result<AiHttpResponse> {
        request_with_ureq(
            "GET",
            url,
            api_key,
            auth,
            proxy_url,
            None,
            timeout_secs,
            &[],
        )
    }
}

/// 按 api_style 组装 chat 请求体。返回 (url_suffix, body)。
pub fn chat_request(
    style: ApiStyle,
    model: &str,
    system: &str,
    user: &str,
    max_tokens: u32,
    stream: bool,
) -> (&'static str, String) {
    match style {
        ApiStyle::OpenAiCompletions => (
            "/chat/completions",
            serde_json::json!({
                "model": model,
                "messages": [
                    { "role": "system", "content": system },
                    { "role": "user", "content": user },
                ],
                "max_tokens": max_tokens,
                "stream": stream,
            })
            .to_string(),
        ),
        ApiStyle::AnthropicMessages => (
            "/v1/messages",
            serde_json::json!({
                "model": model,
                "system": system,
                "messages": [{ "role": "user", "content": user }],
                "max_tokens": max_tokens,
                "stream": stream,
            })
            .to_string(),
        ),
    }
}

/// 从 SSE `data:` 行提取文本增量与可选用量（OpenAI / Anthropic）。
pub fn parse_sse_data_line(style: ApiStyle, data: &str) -> Option<(String, Option<TokenUsage>)> {
    let data = data.trim();
    if data.is_empty() || data == "[DONE]" {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(data).ok()?;
    match style {
        ApiStyle::OpenAiCompletions => {
            let delta = v["choices"]
                .as_array()?
                .first()?
                .get("delta")?
                .get("content")?
                .as_str()?;
            let usage = v
                .get("usage")
                .and_then(|u| serde_json::from_value::<TokenUsage>(u.clone()).ok());
            Some((delta.to_string(), usage))
        }
        ApiStyle::AnthropicMessages => {
            if let Some(text) = v["delta"]["text"].as_str() {
                return Some((text.to_string(), None));
            }
            if v["type"].as_str() == Some("message_delta") {
                let usage = v.get("usage").and_then(|u| {
                    let input = u["input_tokens"].as_u64().unwrap_or(0);
                    let output = u["output_tokens"].as_u64().unwrap_or(0);
                    (input > 0 || output > 0).then(|| TokenUsage {
                        prompt_tokens: input,
                        completion_tokens: output,
                    })
                });
                return usage.map(|u| (String::new(), Some(u)));
            }
            None
        }
    }
}

fn request_stream_with_ureq(
    url: &str,
    api_key: &str,
    auth: AuthMethod,
    proxy_url: Option<&str>,
    body: &str,
    timeout_secs: u64,
    extra_headers: &[(&str, &str)],
    style: ApiStyle,
    on_delta: &mut dyn FnMut(&str),
) -> Result<(String, Option<TokenUsage>)> {
    let proxy =
        match proxy_url.filter(|_| !is_loopback_url(url)) {
            Some(p) => Some(ureq::Proxy::new(normalize_proxy_url(p)?).map_err(|e| {
                Error::new(ErrorCode::AiNotConfigured, format!("代理地址无效: {e}"))
            })?),
            None => None,
        };
    let agent = {
        let mut ab = ureq::AgentBuilder::new().timeout(Duration::from_secs(timeout_secs.max(1)));
        if let Some(p) = proxy {
            ab = ab.proxy(p);
        }
        ab.build()
    };
    let mut builder = agent.post(url);
    if !api_key.is_empty() {
        builder = match auth {
            AuthMethod::ApiKey if url_is_anthropic(url) => builder.set("x-api-key", api_key),
            _ => builder.set("Authorization", &format!("Bearer {api_key}")),
        };
    }
    for (k, v) in extra_headers {
        builder = builder.set(k, v);
    }
    builder = builder.set("Content-Type", "application/json");
    let resp = match builder.send_string(body) {
        Ok(r) => r,
        Err(ureq::Error::Status(code, r)) => {
            let body = r.into_string().unwrap_or_default();
            let snippet: String = body.chars().take(300).collect();
            return Err(Error::new(
                ErrorCode::AiRequestFailed,
                format!("AI 端点返回 {code}: {}", redact_key(&snippet, api_key)),
            ));
        }
        Err(e) => return Err(map_ureq_transport_err(e)),
    };
    let reader = BufReader::new(resp.into_reader());
    let mut full = String::new();
    let mut usage: Option<TokenUsage> = None;
    for line in reader.lines() {
        let line = line.map_err(|e| {
            Error::new(
                ErrorCode::AiRequestFailed,
                format!("AI 流式响应读取失败: {e}"),
            )
        })?;
        let line = line.trim();
        if let Some(rest) = line.strip_prefix("data:") {
            if let Some((delta, u)) = parse_sse_data_line(style, rest) {
                if !delta.is_empty() {
                    full.push_str(&delta);
                    on_delta(&delta);
                }
                if u.is_some() {
                    usage = u;
                }
            }
        }
    }
    if full.is_empty() {
        return Err(Error::new(
            ErrorCode::AiRequestFailed,
            "AI 流式响应未返回任何文本",
        ));
    }
    Ok((full, usage))
}

fn map_ureq_transport_err(e: ureq::Error) -> Error {
    if is_timeout_error(&e) {
        Error::new(ErrorCode::AiTimeout, "AI 请求超时（超过 timeout_secs）")
    } else {
        Error::new(
            ErrorCode::AiRequestFailed,
            format!("AI 端点不可达: {}", transport_message(&e)),
        )
    }
}

/// 解析 2xx 响应体为文本与可选 token 用量。
pub fn parse_chat_response(style: ApiStyle, body: &str) -> Result<(String, Option<TokenUsage>)> {
    match style {
        ApiStyle::OpenAiCompletions => parse_openai_response(body),
        ApiStyle::AnthropicMessages => parse_anthropic_response(body),
    }
}

fn parse_openai_response(body: &str) -> Result<(String, Option<TokenUsage>)> {
    #[derive(Deserialize)]
    struct Choice {
        #[serde(default)]
        message: Option<Message>,
    }
    #[derive(Deserialize)]
    struct Message {
        #[serde(default)]
        content: Option<serde_json::Value>,
    }
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        choices: Vec<Choice>,
        #[serde(default)]
        usage: Option<TokenUsage>,
    }
    let root: Root = serde_json::from_str(body)
        .map_err(|e| Error::new(ErrorCode::AiRequestFailed, format!("AI 响应解析失败: {e}")))?;
    let content = root
        .choices
        .first()
        .and_then(|c| c.message.as_ref())
        .and_then(|m| m.content.as_ref())
        .and_then(|v| match v {
            serde_json::Value::String(s) => Some(s.clone()),
            // 某些兼容端点返回分段数组
            serde_json::Value::Array(parts) => {
                let joined: Vec<String> = parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()).map(str::to_string))
                    .collect();
                (!joined.is_empty()).then(|| joined.join(""))
            }
            _ => None,
        })
        .ok_or_else(|| {
            Error::new(
                ErrorCode::AiRequestFailed,
                "AI 响应缺少 choices[0].message.content",
            )
        })?;
    Ok((content, root.usage))
}

fn parse_anthropic_response(body: &str) -> Result<(String, Option<TokenUsage>)> {
    #[derive(Deserialize)]
    struct Block {
        #[serde(default)]
        text: Option<String>,
    }
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        content: Vec<Block>,
        #[serde(default)]
        usage: Option<AnthropicUsage>,
    }
    #[derive(Deserialize)]
    struct AnthropicUsage {
        #[serde(default)]
        input_tokens: u64,
        #[serde(default)]
        output_tokens: u64,
    }
    let root: Root = serde_json::from_str(body)
        .map_err(|e| Error::new(ErrorCode::AiRequestFailed, format!("AI 响应解析失败: {e}")))?;
    let text: String = root
        .content
        .iter()
        .filter_map(|b| b.text.as_deref())
        .collect::<Vec<_>>()
        .join("");
    if text.is_empty() {
        return Err(Error::new(
            ErrorCode::AiRequestFailed,
            "AI 响应 content 为空",
        ));
    }
    Ok((
        text,
        root.usage.map(|u| TokenUsage {
            prompt_tokens: u.input_tokens,
            completion_tokens: u.output_tokens,
        }),
    ))
}

/// OpenAI 兼容端点模型发现：`GET {base}/models` → 模型 id 列表。
pub fn parse_models_response(body: &str) -> Result<Vec<String>> {
    #[derive(Deserialize)]
    struct Item {
        id: String,
    }
    #[derive(Deserialize)]
    struct Root {
        #[serde(default)]
        data: Vec<Item>,
    }
    let root: Root = serde_json::from_str(body)
        .map_err(|e| Error::new(ErrorCode::AiRequestFailed, format!("模型列表解析失败: {e}")))?;
    Ok(root.data.into_iter().map(|i| i.id).collect())
}

/// 从任意错误/响应文本中抹去 key（dbx `categorized_http_error` 原则：错误不回显凭据）。
pub fn redact_key(text: &str, api_key: &str) -> String {
    if api_key.is_empty() {
        text.to_string()
    } else {
        text.replace(api_key, "<redacted>")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_sse_openai_delta() {
        let line = r#"{"choices":[{"delta":{"content":"你好"}}]}"#;
        let (text, usage) = parse_sse_data_line(ApiStyle::OpenAiCompletions, line).unwrap();
        assert_eq!(text, "你好");
        assert!(usage.is_none());
    }

    #[test]
    fn parse_sse_anthropic_delta() {
        let line = r#"{"type":"content_block_delta","delta":{"text":"Hi"}}"#;
        let (text, _) = parse_sse_data_line(ApiStyle::AnthropicMessages, line).unwrap();
        assert_eq!(text, "Hi");
    }

    #[test]
    fn request_body_openai_shape() {
        let (suffix, body) =
            chat_request(ApiStyle::OpenAiCompletions, "m1", "sys", "user", 512, false);
        assert_eq!(suffix, "/chat/completions");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["model"], "m1");
        assert_eq!(v["max_tokens"], 512);
        assert_eq!(v["stream"], false);
        assert_eq!(v["messages"][0]["role"], "system");
        assert_eq!(v["messages"][1]["content"], "user");
    }

    #[test]
    fn request_body_anthropic_shape() {
        let (suffix, body) =
            chat_request(ApiStyle::AnthropicMessages, "m1", "sys", "user", 512, false);
        assert_eq!(suffix, "/v1/messages");
        let v: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(v["system"], "sys");
        assert_eq!(v["max_tokens"], 512);
        assert_eq!(v["messages"][0]["role"], "user");
    }

    #[test]
    fn parse_response_content_and_usage() {
        let body = serde_json::json!({
            "choices": [{ "message": { "role": "assistant", "content": "你好" } }],
            "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
        })
        .to_string();
        let (text, usage) = parse_chat_response(ApiStyle::OpenAiCompletions, &body).unwrap();
        assert_eq!(text, "你好");
        assert_eq!(
            usage,
            Some(TokenUsage {
                prompt_tokens: 10,
                completion_tokens: 5
            })
        );
    }

    #[test]
    fn parse_response_content_parts_array() {
        let body = serde_json::json!({
            "choices": [{ "message": { "content": [{ "type": "text", "text": "a" }, { "type": "text", "text": "b" }] } }]
        })
        .to_string();
        let (text, usage) = parse_chat_response(ApiStyle::OpenAiCompletions, &body).unwrap();
        assert_eq!(text, "ab");
        assert_eq!(usage, None);
    }

    #[test]
    fn parse_anthropic_blocks_and_usage() {
        let body = serde_json::json!({
            "content": [{ "type": "text", "text": "a" }, { "type": "text", "text": "b" }],
            "usage": { "input_tokens": 11, "output_tokens": 3 }
        })
        .to_string();
        let (text, usage) = parse_chat_response(ApiStyle::AnthropicMessages, &body).unwrap();
        assert_eq!(text, "ab");
        assert_eq!(
            usage,
            Some(TokenUsage {
                prompt_tokens: 11,
                completion_tokens: 3
            })
        );
    }

    #[test]
    fn parse_response_missing_content_is_request_failed() {
        let e = parse_chat_response(ApiStyle::OpenAiCompletions, r#"{"choices":[]}"#).unwrap_err();
        assert_eq!(e.code(), ErrorCode::AiRequestFailed);
        let e2 = parse_chat_response(ApiStyle::OpenAiCompletions, "not json").unwrap_err();
        assert_eq!(e2.code(), ErrorCode::AiRequestFailed);
    }

    #[test]
    fn parse_models_list() {
        let out = parse_models_response(r#"{"data":[{"id":"m2"},{"id":"m1"}]}"#).unwrap();
        assert_eq!(out, vec!["m2".to_string(), "m1".to_string()]);
        assert!(parse_models_response("nope").is_err());
    }

    #[test]
    fn redact_key_replaces_everywhere() {
        let out = redact_key("bad key sk-secret failed at sk-secret/x", "sk-secret");
        assert_eq!(out, "bad key <redacted> failed at <redacted>/x");
        assert_eq!(redact_key("untouched", ""), "untouched");
    }

    #[test]
    fn normalize_proxy_adds_scheme_and_validates() {
        assert_eq!(
            normalize_proxy_url("127.0.0.1:7890").unwrap(),
            "http://127.0.0.1:7890"
        );
        assert_eq!(
            normalize_proxy_url("socks5://h:1080").unwrap(),
            "socks5://h:1080"
        );
        assert!(normalize_proxy_url("ftp://x").is_err());
        assert!(normalize_proxy_url(" ").is_err());
    }

    #[test]
    fn loopback_detection_bypasses_proxy() {
        assert!(is_loopback_url(
            "http://localhost:11434/v1/chat/completions"
        ));
        assert!(is_loopback_url("http://127.0.0.1:8000/v1"));
        assert!(is_loopback_url("http://[::1]:9000/x"));
        assert!(!is_loopback_url("https://api.deepseek.com/v1"));
        assert!(!is_loopback_url("http://internal.host:8000/v1"));
    }
}
