//! 本地 CLI Agent provider：把一次 AI 补全交给本机已安装的编码 CLI
//! （Claude Code / Codex / OpenCode / Cursor / Grok / CodeBuddy / Qoder / Pi）。
//!
//! 与 HTTP provider 的差别：没有 key、没有 base_url，凭据由 CLI 自己管；我们只负责
//! 找到可执行文件、喂 prompt、拿文本。参数默认值参考 dbx 的实现，但存在配置里可改——
//! 各家 CLI 的 flag 会演进，写死等于埋雷。
//!
//! 安全约束：绝不拼 shell（`Command` 直接带 argv），环境变量白名单由用户显式配置，
//! 超时强制 kill，stdout/stderr 只回文本不落盘。

use std::collections::BTreeMap;
use std::io::Write;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

/// 一次 CLI 调用的完整规格（program + argv + env + stdin）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliInvocation {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    /// 经 stdin 传入的 prompt；空串表示不写 stdin。
    pub stdin: String,
    pub timeout_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliOutput {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    /// true = 超时被强制结束（此时 status 无意义）。
    pub timed_out: bool,
}

impl CliOutput {
    pub fn success(&self) -> bool {
        !self.timed_out && self.status == Some(0)
    }
}

/// 进程执行抽象：生产用 [`ProcessCliRunner`]，测试注入假实现。
pub trait CliRunner {
    fn run(&self, invocation: &CliInvocation) -> Result<CliOutput>;
}

pub struct ProcessCliRunner;
struct IsolatedCwd(std::path::PathBuf);
impl IsolatedCwd {
    fn create() -> Result<Self> {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path =
            std::env::temp_dir().join(format!("supertask-ai-{}-{nonce}", std::process::id()));
        std::fs::create_dir(&path).map_err(|e| {
            Error::new(
                ErrorCode::AiRequestFailed,
                format!("创建 CLI 隔离目录失败: {e}"),
            )
        })?;
        Ok(Self(path))
    }
    fn path(&self) -> &std::path::Path {
        &self.0
    }
}
impl Drop for IsolatedCwd {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

impl CliRunner for ProcessCliRunner {
    fn run(&self, invocation: &CliInvocation) -> Result<CliOutput> {
        let workdir = IsolatedCwd::create()?;
        let mut command = Command::new(&invocation.program);
        command
            .args(&invocation.args)
            .current_dir(workdir.path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in &invocation.env {
            command.env(name, value);
        }
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x0800_0000;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let mut child = command.spawn().map_err(|e| {
            Error::new(
                ErrorCode::AiRequestFailed,
                format!("启动 {} 失败: {e}", invocation.program),
            )
        })?;

        if !invocation.stdin.is_empty() {
            if let Some(mut pipe) = child.stdin.take() {
                // CLI 可能在读完 prompt 前就退出（参数错误）；写失败不是致命错误，
                // 真正的诊断信息在 stderr 里。
                let _ = pipe.write_all(invocation.stdin.as_bytes());
            }
        } else {
            drop(child.stdin.take());
        }
        // 未取走的 stdin 句柄会让子进程一直等输入，必须显式关闭。
        drop(child.stdin.take());

        // 输出边读边收：CLI 写满管道缓冲会阻塞，不能先 wait 再读。
        let stdout = child.stdout.take();
        let stderr = child.stderr.take();
        let out_handle = stdout.map(|mut s| {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = std::io::Read::read_to_string(&mut s, &mut buf);
                buf
            })
        });
        let err_handle = stderr.map(|mut s| {
            std::thread::spawn(move || {
                let mut buf = String::new();
                let _ = std::io::Read::read_to_string(&mut s, &mut buf);
                buf
            })
        });

        let deadline = Instant::now() + Duration::from_secs(invocation.timeout_secs.max(1));
        let mut timed_out = false;
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        timed_out = true;
                        break None;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                Err(e) => {
                    return Err(Error::new(
                        ErrorCode::AiRequestFailed,
                        format!("等待 {} 退出失败: {e}", invocation.program),
                    ))
                }
            }
        };

        let stdout = out_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        let stderr = err_handle.and_then(|h| h.join().ok()).unwrap_or_default();
        Ok(CliOutput {
            status: status.and_then(|s| s.code()),
            stdout,
            stderr,
            timed_out,
        })
    }
}

/// CLI 可执行文件校验：只接受“可执行文件路径”，参数走 `cli_args`。
/// 拒绝明显的命令拼接（引号、重定向、管道、分号、换行），空值表示走 PATH 查找。
pub fn validate_cli_path(path: &str) -> Result<Option<String>> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(None);
    }
    const FORBIDDEN: &[char] = &['"', '\'', ';', '|', '&', '<', '>', '\n', '\r', '`', '$'];
    if path.contains(FORBIDDEN) {
        return Err(Error::new(
            ErrorCode::AiNotConfigured,
            "CLI 路径只能填可执行文件路径，命令行参数请填到「CLI 参数」里",
        ));
    }
    Ok(Some(path.to_string()))
}

/// 环境变量名校验（`HTTPS_PROXY` 这类）：避免把整段命令塞进变量名。
pub fn validate_env_name(name: &str) -> Result<()> {
    let ok = !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.chars().next().is_some_and(|c| c.is_ascii_digit());
    if ok {
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::AiNotConfigured,
            format!("环境变量名无效: {name:?}（示例 HTTPS_PROXY）"),
        ))
    }
}

/// 解析出的可执行程序：配置了路径就用它（目录则拼上默认程序名），否则交给 PATH。
pub fn resolve_program(cli_path: Option<&str>, default_program: &str) -> String {
    match cli_path.map(str::trim).filter(|p| !p.is_empty()) {
        Some(path) => {
            let candidate = std::path::Path::new(path);
            if candidate.is_dir() {
                candidate
                    .join(default_program)
                    .to_string_lossy()
                    .to_string()
            } else {
                path.to_string()
            }
        }
        None => default_program.to_string(),
    }
}

pub fn build_env(env: &BTreeMap<String, String>) -> Result<Vec<(String, String)>> {
    let mut out = Vec::with_capacity(env.len());
    for (name, value) in env {
        validate_env_name(name)?;
        out.push((name.clone(), value.clone()));
    }
    Ok(out)
}

/// CLI 探测结果（弹框里显示“已找到 / 未找到”与版本）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CliProbeOut {
    pub program: String,
    pub found: bool,
    pub version: Option<String>,
    /// 未找到时的原因摘要（stderr 首行/超时说明），便于用户自查。
    pub detail: Option<String>,
}

/// 用 `--version` 探测 CLI 是否可用。10s 上限：探测卡住比探测失败更糟。
pub fn probe(
    runner: &dyn CliRunner,
    cli_path: Option<&str>,
    default_program: &str,
    env: &BTreeMap<String, String>,
) -> Result<CliProbeOut> {
    let program = resolve_program(cli_path, default_program);
    let invocation = CliInvocation {
        program: program.clone(),
        args: vec!["--version".to_string()],
        env: build_env(env)?,
        stdin: String::new(),
        timeout_secs: 10,
    };
    match runner.run(&invocation) {
        Ok(out) if out.success() => {
            let version = out
                .stdout
                .lines()
                .map(str::trim)
                .find(|l| !l.is_empty())
                .map(str::to_string);
            Ok(CliProbeOut {
                program,
                found: true,
                version,
                detail: None,
            })
        }
        Ok(out) => Ok(CliProbeOut {
            program,
            found: false,
            version: None,
            detail: Some(if out.timed_out {
                "探测超时（10s）".to_string()
            } else {
                first_line(&out.stderr)
                    .or_else(|| first_line(&out.stdout))
                    .unwrap_or_else(|| format!("退出码 {:?}", out.status))
            }),
        }),
        // 启动失败（不存在/无执行权限）是“未找到”，不是请求错误
        Err(e) => Ok(CliProbeOut {
            program,
            found: false,
            version: None,
            detail: Some(e.message().to_string()),
        }),
    }
}

fn first_line(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .map(str::to_string)
}

/// 从 CLI 输出里提出助手文本。
///
/// 各家格式不同（stream-json / JSONL / 纯文本），且会随版本变；这里按“已知 JSON 形状
/// 优先、纯文本兜底”解析，未识别的 JSON 不会把整段原文当答案吐给用户。
pub fn extract_text(stdout: &str) -> String {
    let mut chunks: Vec<String> = Vec::new();
    let mut saw_json = false;
    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        match serde_json::from_str::<serde_json::Value>(line) {
            Ok(value) => {
                saw_json = true;
                collect_text(&value, &mut chunks);
            }
            Err(_) => {}
        }
    }
    if !chunks.is_empty() {
        return dedupe_join(chunks);
    }
    // 整段 JSON（非 JSONL）
    if let Ok(value) = serde_json::from_str::<serde_json::Value>(stdout.trim()) {
        let mut whole = Vec::new();
        collect_text(&value, &mut whole);
        if !whole.is_empty() {
            return dedupe_join(whole);
        }
        saw_json = true;
    }
    if saw_json {
        // 是 JSON 但没有认识的文本字段：交回原文比装作有答案更诚实
        return stdout.trim().to_string();
    }
    stdout.trim().to_string()
}

/// 累积式流式输出（Claude Code 的 stream-json 会先发增量再发完整 result）会让
/// 同一句话出现两次；后出现的完整文本包含先前片段时只留完整的那一份。
fn dedupe_join(chunks: Vec<String>) -> String {
    let mut kept: Vec<String> = Vec::new();
    for chunk in chunks {
        let chunk = chunk.trim().to_string();
        if chunk.is_empty() {
            continue;
        }
        if kept.iter().any(|k| k == &chunk) {
            continue;
        }
        if let Some(pos) = kept.iter().position(|k| chunk.contains(k.as_str())) {
            kept[pos] = chunk;
            continue;
        }
        if kept.iter().any(|k| k.contains(chunk.as_str())) {
            continue;
        }
        kept.push(chunk);
    }
    kept.join("\n").trim().to_string()
}

/// 已知的助手文本位置。只认 assistant/result 语义的字段，避免把工具调用参数、
/// 思考过程或错误信息当成回答。
fn collect_text(value: &serde_json::Value, out: &mut Vec<String>) {
    let ty = value.get("type").and_then(|v| v.as_str()).unwrap_or("");
    if matches!(ty, "user" | "tool_use" | "tool_result" | "reasoning") {
        return;
    }

    // Claude Code / CodeBuddy / Qoder: {"type":"assistant","message":{"content":[{"type":"text","text":...}]}}
    if let Some(content) = value
        .get("message")
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_array())
    {
        for part in content {
            if part.get("type").and_then(|v| v.as_str()) == Some("text") {
                if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                    push_text(text, out);
                }
            }
        }
    }
    // OpenCode: {"parts":[{"type":"text","text":...}]}
    if let Some(parts) = value.get("parts").and_then(|p| p.as_array()) {
        for part in parts {
            if let Some(text) = part.get("text").and_then(|v| v.as_str()) {
                push_text(text, out);
            }
        }
    }
    // Codex: {"type":"item.completed","item":{"type":"agent_message","text":...}}
    if let Some(item) = value.get("item") {
        let item_type = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
        if item_type.contains("message") || item_type.contains("text") {
            if let Some(text) = item.get("text").and_then(|v| v.as_str()) {
                push_text(text, out);
            }
        }
    }
    // Grok / 通用：{"type":"result","result":"..."} 或 {"text":"..."}
    if ty == "result" || ty == "assistant" || ty.is_empty() {
        if let Some(text) = value.get("result").and_then(|v| v.as_str()) {
            push_text(text, out);
        }
        if let Some(text) = value.get("text").and_then(|v| v.as_str()) {
            push_text(text, out);
        }
        if let Some(text) = value.get("response").and_then(|v| v.as_str()) {
            push_text(text, out);
        }
        if let Some(text) = value.get("content").and_then(|v| v.as_str()) {
            push_text(text, out);
        }
    }
}

fn push_text(text: &str, out: &mut Vec<String>) {
    if !text.trim().is_empty() {
        out.push(text.to_string());
    }
}

/// CLI 失败时的可读错误：优先 stderr 首行，其次 stdout，最后退出码。
pub fn run_error(program: &str, out: &CliOutput) -> Error {
    if out.timed_out {
        return Error::new(
            ErrorCode::AiRequestFailed,
            format!("{program} 超时未返回，可提高超时时间或换更快的模型"),
        );
    }
    let detail = first_line(&out.stderr)
        .or_else(|| first_line(&out.stdout))
        .unwrap_or_default();
    let hint = if detail.is_empty() {
        String::new()
    } else {
        format!("：{detail}")
    };
    Error::new(
        ErrorCode::AiRequestFailed,
        format!("{program} 退出码 {:?}{hint}", out.status),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeRunner {
        out: CliOutput,
    }

    impl CliRunner for FakeRunner {
        fn run(&self, _invocation: &CliInvocation) -> Result<CliOutput> {
            Ok(self.out.clone())
        }
    }

    fn out(stdout: &str, stderr: &str, status: Option<i32>) -> CliOutput {
        CliOutput {
            status,
            stdout: stdout.to_string(),
            stderr: stderr.to_string(),
            timed_out: false,
        }
    }

    #[test]
    fn cli_path_rejects_command_injection_but_keeps_windows_spaces() {
        assert!(validate_cli_path("claude; rm -rf /").is_err());
        assert!(validate_cli_path("claude && whoami").is_err());
        assert!(validate_cli_path("$(which claude)").is_err());
        assert_eq!(
            validate_cli_path(r"C:\Program Files\claude\claude.exe").unwrap(),
            Some(r"C:\Program Files\claude\claude.exe".to_string())
        );
        assert_eq!(validate_cli_path("   ").unwrap(), None);
    }

    #[test]
    fn env_names_must_look_like_env_names() {
        assert!(validate_env_name("HTTPS_PROXY").is_ok());
        assert!(validate_env_name("no_proxy").is_ok());
        assert!(validate_env_name("2FAST").is_err());
        assert!(validate_env_name("FOO=BAR").is_err());
        assert!(validate_env_name("").is_err());
    }

    #[test]
    fn program_falls_back_to_path_lookup() {
        assert_eq!(resolve_program(None, "claude"), "claude");
        assert_eq!(resolve_program(Some("  "), "codex"), "codex");
        assert_eq!(
            resolve_program(Some("/opt/x/claude"), "claude"),
            "/opt/x/claude"
        );
    }

    #[test]
    fn extracts_claude_code_stream_json() {
        let stdout = concat!(
            r#"{"type":"system","subtype":"init"}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"部分"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"部分答案"}]}}"#,
            "\n",
        );
        assert_eq!(extract_text(stdout), "部分答案");
    }

    #[test]
    fn extracts_codex_and_opencode_shapes() {
        let codex =
            r#"{"type":"item.completed","item":{"type":"agent_message","text":"codex 回答"}}"#;
        assert_eq!(extract_text(codex), "codex 回答");
        let opencode = r#"{"parts":[{"type":"text","text":"opencode 回答"}]}"#;
        assert_eq!(extract_text(opencode), "opencode 回答");
    }

    #[test]
    fn plain_text_output_passes_through() {
        assert_eq!(extract_text("  直接文本  \n"), "直接文本");
    }

    #[test]
    fn tool_noise_is_not_mistaken_for_the_answer() {
        let stdout = concat!(
            r#"{"type":"tool_use","message":{"content":[{"type":"text","text":"SELECT 1"}]}}"#,
            "\n",
            r#"{"type":"assistant","message":{"content":[{"type":"text","text":"真答案"}]}}"#,
            "\n",
        );
        assert_eq!(extract_text(stdout), "真答案");
    }

    #[test]
    fn probe_reports_version_when_cli_answers() {
        let runner = FakeRunner {
            out: out("claude 1.2.3\n", "", Some(0)),
        };
        let probe = probe(&runner, None, "claude", &BTreeMap::new()).unwrap();
        assert!(probe.found);
        assert_eq!(probe.version.as_deref(), Some("claude 1.2.3"));
    }

    #[test]
    fn probe_reports_not_found_with_reason() {
        let runner = FakeRunner {
            out: out("", "command not found\n", Some(127)),
        };
        let probe = probe(&runner, None, "nope", &BTreeMap::new()).unwrap();
        assert!(!probe.found);
        assert_eq!(probe.detail.as_deref(), Some("command not found"));
    }

    #[test]
    fn timeout_error_suggests_raising_the_limit() {
        let timed_out = CliOutput {
            status: None,
            stdout: String::new(),
            stderr: String::new(),
            timed_out: true,
        };
        let err = run_error("claude", &timed_out);
        assert!(err.message().contains("超时"));
    }
}
