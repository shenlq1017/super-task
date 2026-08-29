//! compose 解析（规格 §4.3）：不手写 compose schema，`docker compose config --format json`
//! 拿规范化结果；按 mtime + 字节 hash 缓存，spec 打开与端口检查读缓存不重复 spawn。

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use crate::docker::runner::{DockerRunner, DockerSpawn};
use crate::error::{Error, ErrorCode, Result};
use crate::sandbox::confine;

/// ps/config 类命令超时（规格 §4.2）。
const COMMAND_TIMEOUT: Duration = Duration::from_secs(10);

/// compose 解析结果里的单个服务。ports 保持 compose 输出顺序，`port` 取第一个。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeServiceInfo {
    pub name: String,
    pub ports: Vec<u16>,
    pub port: Option<u16>,
    pub depends_on: Vec<String>,
    pub has_build: bool,
    pub has_healthcheck: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ComposeModel {
    pub services: Vec<ComposeServiceInfo>,
}

impl ComposeModel {
    pub fn find(&self, name: &str) -> Option<&ComposeServiceInfo> {
        self.services.iter().find(|s| s.name == name)
    }
}

/// 解析 `config --format json` 输出。不可解析 → `COMPOSE_CONFIG_FAILED`。
pub fn parse_compose_config(stdout: &str) -> Result<ComposeModel> {
    invalid(|| {
        let v = parse_json(stdout)?;
        let services = v
            .get("services")
            .and_then(Value::as_object)
            .ok_or("no services")?;
        let mut out = Vec::with_capacity(services.len());
        for (name, sv) in services {
            let ports: Vec<u16> = sv
                .get("ports")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|p| p.get("published").and_then(published_port))
                        .collect()
                })
                .unwrap_or_default();
            out.push(ComposeServiceInfo {
                name: name.clone(),
                port: ports.first().copied(),
                ports,
                depends_on: depends_on_keys(sv.get("depends_on")),
                has_build: sv.get("build").map(|b| !b.is_null()).unwrap_or(false),
                has_healthcheck: sv.get("healthcheck").map(|h| !h.is_null()).unwrap_or(false),
            });
        }
        // 排序保证输出顺序稳定（JSON 对象顺序是实现细节）。
        out.sort_by(|a, b| a.name.cmp(&b.name));
        if out.is_empty() {
            return Err("compose file has no services".into());
        }
        Ok(ComposeModel { services: out })
    })
}

fn invalid<T>(f: impl FnOnce() -> std::result::Result<T, String>) -> Result<T> {
    f().map_err(|e| {
        Error::new(
            ErrorCode::ComposeConfigFailed,
            format!("compose config 解析失败: {e}"),
        )
    })
}

fn parse_json(stdout: &str) -> std::result::Result<Value, String> {
    serde_json::from_str::<Value>(stdout).map_err(|e| e.to_string())
}

/// `ports[].published`：整数或字符串（"6379"、"8000-8005" 取首段）；null/越界跳过。
fn published_port(v: &Value) -> Option<u16> {
    match v {
        Value::Number(n) => n.as_u64().and_then(|p| u16::try_from(p).ok()),
        Value::String(s) => s.split(['-', ':', '/']).next()?.trim().parse().ok(),
        _ => None,
    }
}

/// 规范化输出中 `depends_on` 是 map（条件对象）或字符串列表；旧版两者都有。
fn depends_on_keys(v: Option<&Value>) -> Vec<String> {
    match v {
        Some(Value::Array(a)) => a
            .iter()
            .filter_map(Value::as_str)
            .map(String::from)
            .collect(),
        Some(Value::Object(o)) => o.keys().cloned().collect(),
        _ => Vec::new(),
    }
}

struct CacheKey {
    root: PathBuf,
    file: String,
    mtime_ms: u64,
    hash: [u8; 32],
}

struct CachedEntry {
    key: CacheKey,
    model: ComposeModel,
}

/// compose config 加载器：单槽缓存（mtime+hash 命中不重新 spawn）。
/// 每个工作区引擎持有一个实例。
pub struct ComposeConfigLoader {
    runner: Arc<dyn DockerRunner>,
    cache: Mutex<Option<CachedEntry>>,
}

impl ComposeConfigLoader {
    pub fn new(runner: Arc<dyn DockerRunner>) -> Self {
        Self {
            runner,
            cache: Mutex::new(None),
        }
    }

    pub fn clear_cache(&self) {
        *self.cache.lock().unwrap() = None;
    }

    /// `compose_file` 相对工作区根（已过 spec 校验；此处 confine 兜底）。
    /// 文件缺失 → `COMPOSE_FILE_MISSING`；docker spawn 失败 → `DOCKER_NOT_FOUND`；
    /// 非零退出/不可解析 → `COMPOSE_CONFIG_FAILED`（stderr 尾部进 message）。
    pub fn load(
        &self,
        root: &Path,
        compose_file: &str,
        project_name: Option<&str>,
    ) -> Result<ComposeModel> {
        let path = confine(root, compose_file)?;
        let meta = std::fs::metadata(&path).map_err(|_| {
            Error::new(
                ErrorCode::ComposeFileMissing,
                format!("compose 文件不存在: {compose_file}"),
            )
        })?;
        let bytes = std::fs::read(&path).map_err(|_| {
            Error::new(
                ErrorCode::ComposeFileMissing,
                format!("compose 文件不可读: {compose_file}"),
            )
        })?;
        let key = CacheKey {
            root: root.to_path_buf(),
            file: compose_file.to_string(),
            mtime_ms: mtime_ms(&meta),
            hash: Sha256::digest(&bytes).into(),
        };
        if let Some(entry) = self.cache.lock().unwrap().as_ref() {
            if entry.key.root == key.root
                && entry.key.file == key.file
                && entry.key.mtime_ms == key.mtime_ms
                && entry.key.hash == key.hash
            {
                return Ok(entry.model.clone());
            }
        }

        let mut args = vec![
            "compose".to_string(),
            "--ansi".to_string(),
            "never".to_string(),
            "-f".to_string(),
            path.display().to_string(),
        ];
        if let Some(p) = project_name {
            args.push("-p".to_string());
            args.push(p.to_string());
        }
        args.push("config".to_string());
        args.push("--format".to_string());
        args.push("json".to_string());

        let out = self
            .runner
            .run(&DockerSpawn {
                args,
                cwd: Some(root.to_path_buf()),
                timeout: COMMAND_TIMEOUT,
            })
            .map_err(|e| {
                if e.kind() == std::io::ErrorKind::NotFound {
                    Error::new(
                        ErrorCode::DockerNotFound,
                        "未找到 docker。请安装 Docker Desktop 并确保在 PATH 中。",
                    )
                } else {
                    Error::new(
                        ErrorCode::ComposeConfigFailed,
                        format!("docker compose config 执行失败: {e}"),
                    )
                }
            })?;
        if out.code != 0 {
            return Err(Error::new(
                ErrorCode::ComposeConfigFailed,
                format!(
                    "docker compose config 退出码 {}: {}",
                    out.code,
                    tail(&out.stderr)
                ),
            ));
        }
        let model = parse_compose_config(&out.stdout)?;
        *self.cache.lock().unwrap() = Some(CachedEntry {
            key,
            model: model.clone(),
        });
        Ok(model)
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// stderr 尾部摘要（daemon 错误一般是最后几行；单行截断由日志管道负责）。
fn tail(s: &str) -> String {
    let trimmed = s.trim();
    const MAX: usize = 400;
    if trimmed.len() <= MAX {
        trimmed.to_string()
    } else {
        let mut start = trimmed.len() - MAX;
        while !trimmed.is_char_boundary(start) {
            start += 1;
        }
        format!("…{}", &trimmed[start..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::runner::FakeDockerRunner;
    use std::io::Write;

    fn fixture_config_json() -> String {
        r#"{
          "name": "mall",
          "services": {
            "redis": {
              "image": "redis:7",
              "ports": [
                {"mode": "ingress", "target": 6379, "published": 6379, "protocol": "tcp"},
                {"mode": "ingress", "target": 16379, "published": "16379", "protocol": "tcp"}
              ],
              "healthcheck": {"test": ["CMD", "redis-cli", "ping"]},
              "build": {"context": "./redis"}
            },
            "mysql": {
              "image": "mysql:8",
              "ports": [{"mode": "ingress", "target": 3306, "published": null, "protocol": "tcp"}],
              "depends_on": {"redis": {"condition": "service_healthy", "required": true}}
            },
            "worker": {
              "image": "mall-worker:dev",
              "depends_on": ["redis", "mysql"],
              "ports": [{"mode": "ingress", "target": 8000, "published": "8000-8005", "protocol": "tcp"}]
            }
          }
        }"#
        .to_string()
    }

    #[test]
    fn parse_extracts_ports_deps_build_health() {
        let model = parse_compose_config(&fixture_config_json()).expect("parse");
        assert_eq!(model.services.len(), 3);

        let redis = model.find("redis").expect("redis");
        assert_eq!(redis.ports, vec![6379, 16379]);
        assert_eq!(redis.port, Some(6379));
        assert!(redis.has_build && redis.has_healthcheck);
        assert!(redis.depends_on.is_empty());

        let mysql = model.find("mysql").expect("mysql");
        // published null → 不算主机端口
        assert!(mysql.ports.is_empty() && mysql.port.is_none());
        assert_eq!(mysql.depends_on, vec!["redis"]);

        let worker = model.find("worker").expect("worker");
        // 端口范围 "8000-8005" 取起始
        assert_eq!(worker.port, Some(8000));
        assert_eq!(worker.depends_on, vec!["redis", "mysql"]);
        assert!(!worker.has_build);
    }

    #[test]
    fn parse_services_sorted_by_name() {
        let model = parse_compose_config(&fixture_config_json()).expect("parse");
        let names: Vec<&str> = model.services.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["mysql", "redis", "worker"]);
    }

    #[test]
    fn parse_invalid_output_maps_compose_config_failed() {
        assert_eq!(
            parse_compose_config("not json").unwrap_err().code(),
            ErrorCode::ComposeConfigFailed
        );
        assert_eq!(
            parse_compose_config(r#"{"other": 1}"#).unwrap_err().code(),
            ErrorCode::ComposeConfigFailed
        );
        assert_eq!(
            parse_compose_config(r#"{"services": {}}"#)
                .unwrap_err()
                .code(),
            ErrorCode::ComposeConfigFailed
        );
    }

    fn write_file(path: &Path, content: &str) {
        let mut f = std::fs::File::create(path).expect("create");
        f.write_all(content.as_bytes()).expect("write");
    }

    fn temp_ws(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-docker-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("mkdir");
        dir
    }

    #[test]
    fn loader_spawns_once_and_caches_by_mtime_hash() {
        let dir = temp_ws("cache");
        write_file(
            &dir.join("compose.yaml"),
            "services:\n  redis:\n    image: redis:7\n",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(fixture_config_json());
        let loader = ComposeConfigLoader::new(fake.clone());

        let m1 = loader.load(&dir, "compose.yaml", None).expect("load 1");
        assert_eq!(m1.services.len(), 3);

        // 命中缓存：不 spawn，不消费脚本
        let m2 = loader
            .load(&dir, "compose.yaml", None)
            .expect("load 2 (cached)");
        assert_eq!(m2, m1);
        assert_eq!(fake.calls().len(), 1);

        // 文件内容变化 → 重新执行
        write_file(
            &dir.join("compose.yaml"),
            "services:\n  redis:\n    image: redis:8\n",
        );
        fake.push_ok(fixture_config_json());
        loader.load(&dir, "compose.yaml", None).expect("load 3");
        assert_eq!(fake.calls().len(), 2);

        // argv：compose --ansi never -f <file> [-p name] config --format json
        let calls = fake.calls();
        assert_eq!(calls[0].args[0], "compose");
        assert_eq!(calls[0].args[1], "--ansi");
        assert_eq!(calls[0].args[2], "never");
        assert_eq!(calls[0].args[3], "-f");
        assert!(calls[0].args[4].ends_with("compose.yaml"));
        assert_eq!(&calls[0].args[5..], &["config", "--format", "json"]);
        assert_eq!(calls[0].cwd.as_deref(), Some(dir.as_path()));
    }

    #[test]
    fn loader_passes_project_name() {
        let dir = temp_ws("proj");
        write_file(
            &dir.join("compose.yaml"),
            "services:\n  redis:\n    image: redis:7\n",
        );
        let fake = Arc::new(FakeDockerRunner::new());
        fake.push_ok(fixture_config_json());
        let loader = ComposeConfigLoader::new(fake.clone());
        loader
            .load(&dir, "compose.yaml", Some("mall"))
            .expect("load");
        let args = &fake.calls()[0].args;
        let p = args.iter().position(|a| a == "-p").expect("-p flag");
        assert_eq!(&args[p..p + 2], &["-p".to_string(), "mall".to_string()]);
    }

    #[test]
    fn loader_maps_missing_file_and_failures() {
        let dir = temp_ws("miss");
        let fake = Arc::new(FakeDockerRunner::new());
        let loader = ComposeConfigLoader::new(fake.clone());

        assert_eq!(
            loader.load(&dir, "compose.yaml", None).unwrap_err().code(),
            ErrorCode::ComposeFileMissing
        );
        assert!(fake.calls().is_empty());

        // docker 不存在
        write_file(
            &dir.join("compose.yaml"),
            "services:\n  redis:\n    image: redis:7\n",
        );
        fake.push_err(std::io::ErrorKind::NotFound);
        assert_eq!(
            loader.load(&dir, "compose.yaml", None).unwrap_err().code(),
            ErrorCode::DockerNotFound
        );

        // 非零退出 → COMPOSE_CONFIG_FAILED，stderr 尾部进 message
        fake.push_fail(1, "service \"redis\" refers to undefined network badnet");
        let err = loader.load(&dir, "compose.yaml", None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ComposeConfigFailed);
        assert!(err.to_string().contains("undefined network"));

        // 输出不可解析 → COMPOSE_CONFIG_FAILED
        fake.push_ok("garbage");
        assert_eq!(
            loader.load(&dir, "compose.yaml", None).unwrap_err().code(),
            ErrorCode::ComposeConfigFailed
        );
    }

    #[test]
    fn loader_rejects_path_escape() {
        let dir = temp_ws("escape");
        let fake = Arc::new(FakeDockerRunner::new());
        let loader = ComposeConfigLoader::new(fake);
        assert_eq!(
            loader
                .load(&dir, "../outside.yaml", None)
                .unwrap_err()
                .code(),
            ErrorCode::PathEscape
        );
    }

    #[test]
    fn tail_truncates_long_stderr() {
        let long = "x".repeat(1000);
        // "…"(1 char) + 末尾 400 字节
        assert_eq!(tail(&long).chars().count(), 401);
        assert!(tail(&long).starts_with('…'));
        assert_eq!(tail("short"), "short");
    }
}
