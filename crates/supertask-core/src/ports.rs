//! 1.2 端口占用与一键改端口（规格 §5）。
//!
//! 检查模型：读取本机 TCP 监听表（`netstat -ano -p tcp`，与仓库「能 spawn
//! 就别内嵌」原则一致；IPv4/IPv6 都解析）。读取失败 → `PORT_SCAN_FAILED`，
//! 绝不把「无法检查」当作「端口可用」。

use std::process::{Command, Stdio};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::spec::SuperTaskFile;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TcpListener {
    pub address: String,
    pub port: u16,
    pub pid: u32,
}

/// 解析 `netstat -ano -p tcp` 输出中的 LISTENING 行。只认 tcp；解析不了的行跳过。
pub fn parse_netstat_listeners(text: &str) -> Vec<TcpListener> {
    let mut out = Vec::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        // Windows 中文/英文输出列数一致：Proto, 本地地址, 远程地址, 状态, PID
        if cols.len() < 5 || !cols[0].eq_ignore_ascii_case("tcp") || !cols[3].eq_ignore_ascii_case("listening") {
            continue;
        }
        let Some((addr, port)) = split_addr_port(cols[1]) else {
            continue;
        };
        let Ok(pid) = cols[4].parse::<u32>() else {
            continue;
        };
        out.push(TcpListener { address: addr, port, pid });
    }
    out
}

/// `127.0.0.1:8081` / `[::]:6379` → (地址, 端口)。
fn split_addr_port(s: &str) -> Option<(String, u16)> {
    if let Some(rest) = s.strip_prefix('[') {
        // IPv6：[::]:port / [2001:db8::1]:80
        let (host, port) = rest.split_once("]:")?;
        Some((format!("[{host}]"), port.parse().ok()?))
    } else {
        let (host, port) = s.rsplit_once(':')?;
        Some((host.to_string(), port.parse().ok()?))
    }
}

fn run_netstat() -> Result<String> {
    let mut cmd = Command::new("netstat");
    cmd.args(["-ano", "-p", "tcp"]).stdout(Stdio::piped()).stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| {
        Error::new(ErrorCode::PortScanFailed, format!("无法读取端口表（netstat）: {e}"))
    })?;
    if !out.status.success() {
        return Err(Error::new(ErrorCode::PortScanFailed, "netstat 非零退出，端口表不可用"));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn tcp_listeners() -> Result<Vec<TcpListener>> {
    Ok(parse_netstat_listeners(&run_netstat()?))
}

/// §5.1：每个有 `port` 的服务一条。`managed` = listener PID 属于本引擎 Job。
pub fn inspect(
    spec: &SuperTaskFile,
    listeners: &[TcpListener],
    managed_pids: &std::collections::HashSet<u32>,
) -> Vec<crate::ipc::PortInspection> {
    let mut items = Vec::new();
    for (id, svc) in &spec.services {
        let Some(port) = svc.port else { continue };
        let hit: Vec<&TcpListener> = listeners.iter().filter(|l| l.port == port).collect();
        let in_use = !hit.is_empty();
        let (pid, process, managed) = match hit.first() {
            Some(l) => (
                Some(l.pid),
                process_name_of(l.pid),
                managed_pids.contains(&l.pid),
            ),
            None => (None, None, false),
        };
        items.push(crate::ipc::PortInspection {
            id: id.clone(),
            port,
            in_use,
            pid,
            process_name: process,
            managed,
        });
    }
    items
}

#[cfg(windows)]
fn process_name_of(pid: u32) -> Option<String> {
    // tasklist /fi "PID eq N" /fo csv /nh 输出："java.exe","4120",...
    let mut cmd = Command::new("tasklist");
    cmd.args(["/fi", &format!("PID eq {pid}"), "/fo", "csv", "/nh"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let out = cmd.output().ok()?;
    let text = String::from_utf8_lossy(&out.stdout);
    let line = text.lines().next()?.trim();
    if line.starts_with("\"") {
        line.split(',').next()?.trim_matches('"').to_string().into()
    } else {
        None
    }
}

#[cfg(not(windows))]
fn process_name_of(_pid: u32) -> Option<String> {
    None
}

/// §5.2：从当前端口向上扫，跳过其他服务的 port/ports、系统保留段与已发现监听；
/// 最多检查 128 个候选、返回最多 5 个。
pub fn suggest(
    spec: &SuperTaskFile,
    id: &str,
    listeners: &[TcpListener],
) -> Result<Vec<u16>> {
    let svc = spec
        .services
        .get(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    let current = svc
        .port
        .ok_or_else(|| Error::new(ErrorCode::SpecInvalid, format!("{id} 没有配置 port")))?;
    let mut used: std::collections::HashSet<u16> = listeners.iter().map(|l| l.port).collect();
    for (other_id, other) in &spec.services {
        if other_id == id {
            continue;
        }
        if let Some(p) = other.port {
            used.insert(p);
        }
        for p in &other.ports {
            used.insert(*p);
        }
    }
    let mut out = Vec::new();
    let mut checked = 0u32;
    let mut candidate = current.saturating_add(1);
    while out.len() < 5 && checked < 128 {
        checked += 1;
        if candidate < 1024 || used.contains(&candidate) {
            candidate = candidate.saturating_add(1);
            if candidate == 0 {
                break;
            }
            continue;
        }
        out.push(candidate);
        used.insert(candidate); // 多个候选不重复
        candidate = candidate.saturating_add(1);
        if candidate == 0 {
            break;
        }
    }
    if out.is_empty() {
        return Err(Error::new(
            ErrorCode::PortNoAvailable,
            format!("在 {current} 之后 128 个候选内没有可用端口"),
        ));
    }
    Ok(out)
}

/// §5.3 配置写回规则（纯函数，改前克隆）。返回需要提示给用户的说明。
pub fn apply_port_assign(
    spec: &mut SuperTaskFile,
    id: &str,
    new_port: u16,
) -> Result<Vec<String>> {
    let svc = spec
        .services
        .get_mut(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    let Some(old_port) = svc.port else {
        return Err(Error::new(ErrorCode::SpecInvalid, format!("{id} 没有配置 port")));
    };
    let mut notes = Vec::new();
    svc.port = Some(new_port);

    // 端口注入键：存在且等于旧端口 → 跟随更新；存在但不同 → 保留并提示
    let key = match svc.kind.as_str() {
        "spring-boot" => "SERVER_PORT",
        "node" => "PORT",
        _ => "",
    };
    if !key.is_empty() {
        if let Some(v) = svc.env.get_mut(key) {
            if v == &old_port.to_string() {
                *v = new_port.to_string();
            } else {
                notes.push(format!("显式环境变量 {key}={v} 未改（与端口不一致，保留原值）"));
            }
        }
    }

    // health http：默认 loopback URL 且端口为旧端口 → 更新；自定义 URL 保留并提示
    if let Some(h) = &mut svc.health {
        if let Some(url) = &mut h.http {
            let old_authority = ["http://127.0.0.1:", &old_port.to_string()].concat();
            let old_localhost = ["http://localhost:", &old_port.to_string()].concat();
            if url.starts_with(&old_authority) {
                *url = format!("http://127.0.0.1:{new_port}{}", &url[old_authority.len()..]);
            } else if url.starts_with(&old_localhost) {
                *url = format!("http://localhost:{new_port}{}", &url[old_localhost.len()..]);
            } else if url.contains(&format!(":{old_port}")) {
                notes.push(format!("自定义健康 URL 未改: {url}"));
            }
        }
    }
    Ok(notes)
}

/// 服务 env 合并需要用到的注入键（engine 侧环境链构建用）。
pub fn port_env_key(kind: &str) -> Option<&'static str> {
    match kind {
        "spring-boot" => Some("SERVER_PORT"),
        "node" => Some("PORT"),
        _ => None,
    }
}

pub fn empty_env() -> IndexMap<String, String> {
    IndexMap::new()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;

    const NETSTAT: &str = "\n  协议  本地地址          外部地址        状态           PID\n  TCP    127.0.0.1:8081           0.0.0.0:0              LISTENING       4120\n  TCP    0.0.0.0:5432             0.0.0.0:0              LISTENING       999\n  TCP    [::]:6379                [::]:0                 LISTENING       1234\n  TCP    127.0.0.1:9000           1.2.3.4:55             ESTABLISHED     77\n";

    fn spec(yaml: &str) -> SuperTaskFile {
        parse_yaml(yaml).unwrap().0
    }

    #[test]
    fn netstat_parse_v4_v6_and_states() {
        let ls = parse_netstat_listeners(NETSTAT);
        assert_eq!(ls.len(), 3);
        assert_eq!(ls[0], TcpListener { address: "127.0.0.1".into(), port: 8081, pid: 4120 });
        assert_eq!(ls[1].port, 5432);
        assert_eq!(ls[2].address, "[::]".to_string());
        assert_eq!(ls[2].port, 6379);
    }

    #[test]
    fn inspect_marks_in_use_and_managed() {
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8081\n  db:\n    kind: node\n    dir: db\n    port: 6379\n";
        let s = spec(y);
        let mut managed = std::collections::HashSet::new();
        managed.insert(1234u32);
        let items = inspect(&s, &parse_netstat_listeners(NETSTAT), &managed);
        assert_eq!(items.len(), 2);
        let api = items.iter().find(|i| i.id == "api").unwrap();
        assert!(api.in_use && !api.managed && api.pid == Some(4120));
        // process_name 走真实 tasklist，不按内容断言（本机进程表不可预测）
        let db = items.iter().find(|i| i.id == "db").unwrap();
        assert!(db.in_use && db.managed && db.pid == Some(1234));
    }

    #[test]
    fn suggest_skips_used_and_reserved() {
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 8081\n";
        let s = spec(y);
        let cands = suggest(&s, "api", &parse_netstat_listeners(NETSTAT)).unwrap();
        assert_eq!(cands.len(), 5);
        assert!(!cands.contains(&8081), "跳过兄弟服务端口");
        assert!(cands.iter().all(|c| *c >= 1024));
        // 8080 的下一个可用：8081 被兄弟占用 → 8082 起
        assert_eq!(cands[0], 8082);
    }

    #[test]
    fn assign_updates_env_and_default_health_url_only() {
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n    env:\n      SERVER_PORT: \"8080\"\n      OTHER: keep\n    health:\n      type: http\n      http: http://127.0.0.1:8080/actuator/health\n  web:\n    kind: node\n    dir: web\n    port: 5173\n    env:\n      PORT: \"9999\"\n    health:\n      type: tcp\n";
        let mut s = spec(y);
        let notes = apply_port_assign(&mut s, "api", 9090).unwrap();
        let api = s.services.get("api").unwrap();
        assert_eq!(api.port, Some(9090));
        assert_eq!(api.env.get("SERVER_PORT").map(String::as_str), Some("9090"));
        assert_eq!(api.env.get("OTHER").map(String::as_str), Some("keep"));
        assert_eq!(
            api.health.as_ref().unwrap().http.as_deref(),
            Some("http://127.0.0.1:9090/actuator/health")
        );
        assert!(notes.is_empty());

        // web：PORT 显式值与旧端口不同 → 保留并提示
        let notes2 = apply_port_assign(&mut s, "web", 5174).unwrap();
        let web = s.services.get("web").unwrap();
        assert_eq!(web.env.get("PORT").map(String::as_str), Some("9999"));
        assert!(notes2.iter().any(|n| n.contains("PORT=9999")));
    }
}
