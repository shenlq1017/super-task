//! 只读命令（1.5 §4.1）：status / logs / doctor / version。不取工作区锁。

use std::path::Path;

use supertask_core::engine::load_yaml_at;
use supertask_core::lock::{self, LockInfo};
use supertask_core::proc;

use crate::output;

/// status 数据（`--json` 结构由测试锁定）。state 来自端口探测——
/// 只读进程看不到 owner 引擎内部状态机，端口监听是 CLI 语义下能给出的真话。
pub fn status_data(root: &Path) -> Result<serde_json::Value, supertask_core::Error> {
    let (_, _, spec, _) = load_yaml_at(root)?;
    let lock = lock::query(root);
    let mut services = Vec::new();
    for (id, svc) in &spec.services {
        let listening = svc.port.map(supertask_core::ports::is_serving).unwrap_or(false);
        services.push(serde_json::json!({
            "id": id,
            "kind": svc.kind,
            "port": svc.port,
            "listening": listening,
            "state": if listening { "running" } else { "stopped" },
        }));
    }
    Ok(serde_json::json!({
        "workspace": root.display().to_string(),
        "lock": lock.map(|l| lock_value(&l)),
        "services": services,
    }))
}

fn lock_value(l: &LockInfo) -> serde_json::Value {
    serde_json::json!({
        "holder": l.holder.as_str(),
        "pid": l.pid,
        "alive": proc::pid_alive(l.pid),
    })
}

pub fn run_status(json: bool, root: &Path) -> Result<i32, supertask_core::Error> {
    let data = status_data(root)?;
    if !json {
        let owner = match data["lock"].as_object() {
            Some(o) => format!(
                "owner: {} (pid {}){}",
                o["holder"].as_str().unwrap_or("?"),
                o["pid"],
                if o["alive"].as_bool().unwrap_or(false) { "" } else { " [stale]" }
            ),
            None => "owner: 无".to_string(),
        };
        println!("工作区 {}", data["workspace"].as_str().unwrap_or("?"));
        println!("  {owner}");
        for svc in data["services"].as_array().unwrap_or(&Vec::new()) {
            println!(
                "  {:<20} {:<12} {:>7}  {}",
                svc["id"].as_str().unwrap_or("?"),
                svc["kind"].as_str().unwrap_or("?"),
                svc["port"]
                    .as_u64()
                    .map(|p| format!(":{p}"))
                    .unwrap_or_else(|| "-".to_string()),
                svc["state"].as_str().unwrap_or("?"),
            );
        }
    }
    output::ok(json, data);
    Ok(output::EXIT_OK)
}

/// logs 数据（CLI `logs` 与 MCP `supertask_logs` 共用；`--json` 结构由测试锁定）。
pub fn logs_data(
    root: &Path,
    id: Option<&str>,
    lines: usize,
    grep: Option<&str>,
) -> Result<serde_json::Value, supertask_core::Error> {
    use supertask_core::ipc::{LogSource, LogSourceKind};
    let source = id.map(|i| LogSource { kind: LogSourceKind::Service, id: i.to_string() });
    let result = match grep {
        Some(q) => serde_json::to_value(supertask_core::log::search_logs(
            root,
            source.as_ref(),
            q,
            false,
            Some(lines),
        )?),
        None => serde_json::to_value(supertask_core::log::tail_logs(root, source.as_ref(), lines)?),
    };
    result.map_err(|e| supertask_core::Error::new(supertask_core::ErrorCode::LogQueryInvalid, format!("序列化失败: {e}")))
}

pub fn run_logs(
    json: bool,
    root: &Path,
    id: Option<&str>,
    lines: usize,
    grep: Option<&str>,
) -> Result<i32, supertask_core::Error> {
    let data = logs_data(root, id, lines, grep)?;
    if !json {
        for hit in data["items"].as_array().unwrap_or(&Vec::new()) {
            println!("[{}] {}", hit["id"].as_str().unwrap_or("?"), hit["text"].as_str().unwrap_or(""));
        }
        if data["truncated"].as_bool().unwrap_or(false) {
            println!("（仅显示最后 {lines} 行）");
        }
    }
    output::ok(json, data);
    Ok(output::EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_shape_is_locked() {
        let root = std::env::temp_dir().join(format!("st-cli-status-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("supertask.yaml"),
            "version: 1\nname: t\nservices:\n  api:\n    kind: spring-boot\n    module: m\n    port: 18080\n",
        )
        .unwrap();
        let data = status_data(&root).unwrap();
        assert_eq!(data["services"][0]["id"], "api");
        assert_eq!(data["services"][0]["kind"], "spring-boot");
        assert_eq!(data["services"][0]["port"], 18080);
        assert_eq!(data["services"][0]["state"], "stopped");
        assert!(data["lock"].is_null(), "no lock in a fresh workspace");
        assert_eq!(data["workspace"], root.display().to_string());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn error_envelope_uses_ipc_code_table() {
        let e = supertask_core::Error::new(
            supertask_core::ErrorCode::WorkspaceLocked,
            "x",
        );
        let v = output::error_value(&e);
        assert_eq!(v["code"], "WORKSPACE_LOCKED");
        assert_eq!(v["message"], "x");
    }
}

pub fn run_doctor(json: bool) -> Result<i32, supertask_core::Error> {
    let toolchain = supertask_core::probe_toolchain();
    let docker = supertask_core::docker::probe_docker(&supertask_core::docker::ProcessDockerRunner);
    let data = serde_json::json!({ "toolchain": toolchain, "docker": docker });
    if !json {
        for (name, t) in [
            ("java", &toolchain.java),
            ("maven", &toolchain.maven),
            ("gradle", &toolchain.gradle),
            ("node", &toolchain.node),
            ("npm", &toolchain.npm),
            ("pnpm", &toolchain.pnpm),
            ("yarn", &toolchain.yarn),
        ] {
            if t.found {
                println!(
                    "  {:<8} {} {}",
                    name,
                    t.version.as_deref().unwrap_or("?"),
                    t.path.as_deref().unwrap_or("")
                );
            } else {
                println!("  {name:<8} 未找到");
            }
        }
        if docker.found {
            println!(
                "  docker   {} compose {} daemon {}",
                docker.version.as_deref().unwrap_or("?"),
                docker.compose_version.as_deref().unwrap_or("插件缺失"),
                if docker.running { "运行中" } else { "未运行" },
            );
        } else {
            println!("  docker   未找到");
        }
    }
    output::ok(json, data);
    Ok(output::EXIT_OK)
}

pub fn run_version(json: bool) -> Result<i32, supertask_core::Error> {
    let data = serde_json::json!({
        "product_version": env!("CARGO_PKG_VERSION"),
        "engine_version": env!("CARGO_PKG_VERSION"),
        "protocol": supertask_core::ipc::PROTOCOL,
    });
    if !json {
        println!(
            "supertask {}（protocol {}, engine supertask-core {}）",
            data["product_version"].as_str().unwrap_or("?"),
            data["protocol"],
            data["engine_version"].as_str().unwrap_or("?"),
        );
    }
    output::ok(json, data);
    Ok(output::EXIT_OK)
}
