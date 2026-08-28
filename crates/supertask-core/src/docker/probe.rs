//! Docker 探测（规格 §4.1）：`docker version --format json` + `docker compose version`。
//!
//! 三态 → 错误码映射（§10.1）：
//! PATH 无 docker → `DOCKER_NOT_FOUND`；daemon 未运行 → `DOCKER_ENGINE_UNREACHABLE`；
//! 无 compose 插件 → `DOCKER_COMPOSE_MISSING`。探测不修改 `DOCKER_HOST`/context。

use std::io;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::docker::runner::{DockerRunner, DockerSpawn};
use crate::error::{Error, ErrorCode, Result};

/// 探测超时：健康 daemon <150ms；挂死的 docker.exe 在此被杀并按未运行处理。
const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct DockerProbe {
    /// PATH 上有 docker 可执行文件。
    pub found: bool,
    /// `docker version` Client.Version（如 "27.1.1"）。
    pub version: Option<String>,
    /// compose 插件版本（如 "2.29.1"，去 `v` 前缀）；插件缺失时为 None。
    pub compose_version: Option<String>,
    /// daemon 存活（Server 段存在）。
    pub running: bool,
}

pub fn probe_docker(runner: &dyn DockerRunner) -> DockerProbe {
    let spec = DockerSpawn {
        args: vec!["version".into(), "--format".into(), "json".into()],
        cwd: None,
        timeout: PROBE_TIMEOUT,
    };
    let out = match runner.run(&spec) {
        Ok(o) => o,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return DockerProbe::default(),
        // 其他 spawn 失败（权限等）同样按「不可用」上报，不让 UI 卡死。
        Err(_) => return DockerProbe::default(),
    };
    let parsed = parse_version_json(&out.stdout);
    let running = parsed.server_present;
    let compose_version = if running {
        probe_compose_version(runner)
    } else {
        None
    };
    DockerProbe {
        found: true,
        version: parsed.version,
        compose_version,
        running,
    }
}

/// daemon 存活时查 compose 插件。新版 compose 返回 `{"version":"v5.4.0"}`，
/// 旧版返回 `{"ComposeVersion":"v2.29.1"}`；更旧的 compose 不支持 `--format json`
/// （非零退出）时兜底 `docker compose version` 纯文本解析。
fn probe_compose_version(runner: &dyn DockerRunner) -> Option<String> {
    let json_spec = DockerSpawn {
        args: vec!["compose".into(), "version".into(), "--format".into(), "json".into()],
        cwd: None,
        timeout: PROBE_TIMEOUT,
    };
    if let Ok(out) = runner.run(&json_spec) {
        if out.code == 0 {
            if let Some(v) = parse_compose_version_json(&out.stdout) {
                return Some(v);
            }
        }
        // --format json 失败（旧版未知 flag）：stdout 若已带版本文本直接用
        if let Some(v) = parse_compose_version_text(&out.stdout) {
            return Some(v);
        }
    }
    let plain_spec = DockerSpawn {
        args: vec!["compose".into(), "version".into()],
        cwd: None,
        timeout: PROBE_TIMEOUT,
    };
    let out = runner.run(&plain_spec).ok()?;
    if out.code != 0 {
        return None;
    }
    parse_compose_version_text(&out.stdout)
}

struct ParsedVersion {
    version: Option<String>,
    server_present: bool,
}

/// `docker version --format json`：`{"Client":{"Version":"27.1.1"},"Server":{...}}`；
/// daemon 未运行时 Server 为 null/缺失（退出码 1）。容忍旧版输出非 JSON（→ 不可解析视为 found 但 unknown）。
fn parse_version_json(stdout: &str) -> ParsedVersion {
    match serde_json::from_str::<Value>(stdout) {
        Ok(v) => ParsedVersion {
            version: v
                .pointer("/Client/Version")
                .and_then(Value::as_str)
                .map(str::to_string),
            server_present: v.get("Server").map(|s| !s.is_null()).unwrap_or(false),
        },
        // 非 JSON 输出：docker 存在但拿不到结构化版本；按 found + 未运行处理，
        // 后续 compose 命令仍会给出真实错误。
        Err(_) => ParsedVersion {
            version: None,
            server_present: false,
        },
    }
}

/// compose 版本 JSON：旧键 `ComposeVersion`（`{"ComposeVersion":"v2.29.1"}`）、
/// 新键 `version`（docker 29+/compose v5：`{"version":"v5.4.0"}`）。
fn parse_compose_version_json(stdout: &str) -> Option<String> {
    let v: Value = serde_json::from_str(stdout).ok()?;
    let s = v
        .get("ComposeVersion")
        .or_else(|| v.get("version"))?
        .as_str()?;
    normalize_compose_version(s)
}

/// 兜底文本：`Docker Compose version v2.29.1`。要求行内出现
/// "compose version" 字样，避免把任意 stderr 文本当版本号。
fn parse_compose_version_text(stdout: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)compose version\s+v?(\d+(?:\.\d+)*)").ok()?;
    re.captures(stdout)?
        .get(1)
        .map(|m| m.as_str().to_string())
}

fn normalize_compose_version(s: &str) -> Option<String> {
    let t = s.trim().trim_start_matches(['v', 'V']);
    if t.is_empty() {
        None
    } else {
        Some(t.to_string())
    }
}

/// compose 相关操作前的同步前置检查（规格 §2.4 / §5.2）。
pub fn ensure_compose_ready(probe: &DockerProbe) -> Result<()> {
    if !probe.found {
        return Err(Error::new(
            ErrorCode::DockerNotFound,
            "未找到 docker。请安装 Docker Desktop 并确保在 PATH 中。",
        ));
    }
    if !probe.running {
        return Err(Error::new(
            ErrorCode::DockerEngineUnreachable,
            "Docker 引擎未运行。请启动 Docker Desktop 后重试探测。",
        ));
    }
    if probe.compose_version.is_none() {
        return Err(Error::new(
            ErrorCode::DockerComposeMissing,
            "docker compose 插件不可用。请升级 Docker Desktop 或单独安装 compose 插件。",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::runner::{DockerOutput, FakeDockerRunner};

    fn version_json(server: bool) -> String {
        let server_part = if server {
            r#","Server":{"Version":"27.1.1","Components":[{"Name":"engine","Version":"27.1.1"}]}"#
        } else {
            r#","Server":null"#
        };
        format!(
            r#"{{"Client":{{"Version":"27.1.1","ApiVersion":"1.46","Os":"windows"}}{server_part}}}"#
        )
    }

    #[test]
    fn parses_version_json_with_server() {
        let p = parse_version_json(&version_json(true));
        assert_eq!(p.version.as_deref(), Some("27.1.1"));
        assert!(p.server_present);
    }

    #[test]
    fn parses_version_json_server_null_means_not_running() {
        let p = parse_version_json(&version_json(false));
        assert_eq!(p.version.as_deref(), Some("27.1.1"));
        assert!(!p.server_present);
    }

    #[test]
    fn non_json_output_is_found_but_not_running() {
        let p = parse_version_json("Client:\n Version: 27.1.1");
        assert_eq!(p.version, None);
        assert!(!p.server_present);
    }

    #[test]
    fn compose_version_from_json_and_text() {
        // 旧键（compose v2）
        assert_eq!(
            parse_compose_version_json(r#"{"ComposeVersion":"v2.29.1"}"#).as_deref(),
            Some("2.29.1")
        );
        // 新键（docker 29+ / compose v5，2026-08 真机实测）
        assert_eq!(
            parse_compose_version_json(r#"{"version":"v5.4.0"}"#).as_deref(),
            Some("5.4.0")
        );
        assert_eq!(
            parse_compose_version_text("Docker Compose version v2.29.1").as_deref(),
            Some("2.29.1")
        );
        assert_eq!(parse_compose_version_text("garbage"), None);
    }

    #[test]
    fn compose_version_falls_back_to_plain_text_when_json_flag_unsupported() {
        // 旧版 compose 不认识 --format json：非零退出 → 兜底纯文本探测
        let fake = FakeDockerRunner::new();
        fake.push_ok(version_json(true));
        fake.push_fail(1, "unknown flag: --format");
        fake.push_ok("Docker Compose version v2.21.0\n");
        let probe = probe_docker(&fake);
        assert_eq!(probe.compose_version.as_deref(), Some("2.21.0"));
        let calls = fake.calls();
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[2].args, vec!["compose", "version"]);
    }

    #[test]
    fn compose_new_key_json_on_real_output() {
        // 2026-08 真机实测：`docker compose version --format json`（compose v5.4.0）
        let fake = FakeDockerRunner::new();
        fake.push_ok(version_json(true));
        fake.push_ok("{\"version\":\"v5.4.0\"}");
        let probe = probe_docker(&fake);
        assert_eq!(probe.compose_version.as_deref(), Some("5.4.0"));
        assert!(ensure_compose_ready(&probe).is_ok());
        assert_eq!(fake.calls().len(), 2);
    }

    #[test]
    fn probe_maps_three_states() {
        // 1) PATH 无 docker
        let fake = FakeDockerRunner::new();
        fake.push_err(io::ErrorKind::NotFound);
        let probe = probe_docker(&fake);
        assert!(!probe.found && !probe.running);
        assert_eq!(ensure_compose_ready(&probe).unwrap_err().code(), ErrorCode::DockerNotFound);

        // 2) Docker Desktop 已装未运行：version 退出 1，Server null
        let fake = FakeDockerRunner::new();
        fake.push_ok(version_json(false));
        let probe = probe_docker(&fake);
        assert!(probe.found && !probe.running);
        assert_eq!(probe.version.as_deref(), Some("27.1.1"));
        assert_eq!(ensure_compose_ready(&probe).unwrap_err().code(), ErrorCode::DockerEngineUnreachable);

        // 3) 有 docker 有 daemon，无 compose 插件（compose version 退出 1）
        let fake = FakeDockerRunner::new();
        fake.push_ok(version_json(true));
        fake.push_fail(1, "docker: 'compose' is not a docker command.");
        let probe = probe_docker(&fake);
        assert!(probe.found && probe.running && probe.compose_version.is_none());
        assert_eq!(ensure_compose_ready(&probe).unwrap_err().code(), ErrorCode::DockerComposeMissing);
    }

    #[test]
    fn probe_ready_when_all_present() {
        let fake = FakeDockerRunner::new();
        fake.push_ok(version_json(true));
        fake.push_ok(r#"{"ComposeVersion":"v2.29.1"}"#);
        let probe = probe_docker(&fake);
        assert_eq!(probe.compose_version.as_deref(), Some("2.29.1"));
        assert!(probe.running);
        assert!(ensure_compose_ready(&probe).is_ok());

        // compose 探测只跑一次且 argv 固定
        let calls = fake.calls();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0].args, vec!["version", "--format", "json"]);
        assert_eq!(calls[1].args, vec!["compose", "version", "--format", "json"]);
    }

    #[test]
    fn probe_skips_compose_check_when_daemon_down() {
        let fake = FakeDockerRunner::new();
        fake.push_ok(version_json(false));
        let probe = probe_docker(&fake);
        assert!(probe.compose_version.is_none());
        assert_eq!(fake.calls().len(), 1);
    }

    #[test]
    fn output_type_round_trip() {
        let out = DockerOutput {
            code: 0,
            stdout: "{}".into(),
            stderr: String::new(),
            truncated: false,
        };
        assert_eq!(out.code, 0);
    }
}
