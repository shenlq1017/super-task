//! 1.2 端口占用与一键改端口（规格 §5）。
//!
//! 检查模型：读取本机 TCP 监听表（`netstat -ano -p tcp`，与仓库「能 spawn
//! 就别内嵌」原则一致；IPv4/IPv6 都解析）。读取失败 → `PORT_SCAN_FAILED`，
//! 绝不把「无法检查」当作「端口可用」。

use std::process::{Command, Stdio};
use std::time::Duration;

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::spec::SuperTaskFile;

/// loopback:port 是否已有服务在监听（250ms 上限；open / CLI status 批量调用要快）。
/// 双栈：Node/Vite 默认常只监听 [::1]，仅探 IPv4 会把外部运行的服务误判为未启动。
pub fn is_serving(port: u16) -> bool {
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
    [
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        IpAddr::V6(Ipv6Addr::LOCALHOST),
    ]
    .iter()
    .any(|ip| {
        TcpStream::connect_timeout(&SocketAddr::new(*ip, port), Duration::from_millis(250)).is_ok()
    })
}

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
        if cols.len() < 5
            || !cols[0].eq_ignore_ascii_case("tcp")
            || !cols[3].eq_ignore_ascii_case("listening")
        {
            continue;
        }
        let Some((addr, port)) = split_addr_port(cols[1]) else {
            continue;
        };
        let Ok(pid) = cols[4].parse::<u32>() else {
            continue;
        };
        out.push(TcpListener {
            address: addr,
            port,
            pid,
        });
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
    cmd.args(["-ano", "-p", "tcp"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let out = cmd.output().map_err(|e| {
        Error::new(
            ErrorCode::PortScanFailed,
            format!("无法读取端口表（netstat）: {e}"),
        )
    })?;
    if !out.status.success() {
        return Err(Error::new(
            ErrorCode::PortScanFailed,
            "netstat 非零退出，端口表不可用",
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

pub fn tcp_listeners() -> Result<Vec<TcpListener>> {
    #[cfg(windows)]
    {
        Ok(parse_netstat_listeners(&run_netstat()?))
    }
    #[cfg(not(windows))]
    {
        unix_listeners()
    }
}

/// 1.4 §4.4：Linux 读 `/proc/net/tcp{,6}` + `/proc/<pid>/fd` 关联 PID；
/// macOS spawn `lsof -nP -iTCP -sTCP:LISTEN`（系统自带）。读不到 → `PORT_SCAN_FAILED`，
/// 不把「无法检查」当「端口可用」（口径与 Windows 一致）。
#[cfg(not(windows))]
fn unix_listeners() -> Result<Vec<TcpListener>> {
    #[cfg(target_os = "linux")]
    {
        linux_listeners()
    }
    #[cfg(not(target_os = "linux"))]
    {
        macos_listeners()
    }
}

#[cfg(target_os = "linux")]
fn linux_listeners() -> Result<Vec<TcpListener>> {
    fn read_small(path: &str) -> std::io::Result<String> {
        Ok(String::from_utf8_lossy(&std::fs::read(path)?).into_owned())
    }
    let v4 = read_small("/proc/net/tcp").map_err(|e| {
        Error::new(
            ErrorCode::PortScanFailed,
            format!("无法读取 /proc/net/tcp: {e}"),
        )
    })?;
    let v6 = read_small("/proc/net/tcp6").map_err(|e| {
        Error::new(
            ErrorCode::PortScanFailed,
            format!("无法读取 /proc/net/tcp6: {e}"),
        )
    })?;
    let mut rows = parse_proc_net_tcp(&v4);
    rows.extend(parse_proc_net_tcp(&v6));

    // inode → pid：扫 /proc/<pid>/fd 的 socket:[inode] 链接
    let mut inode_pid: std::collections::HashMap<u64, u32> = std::collections::HashMap::new();
    if let Ok(all) = procfs::process::all_processes() {
        for p in all.into_iter().filter_map(|p| p.ok()) {
            let pid = p.pid as u32;
            if let Ok(fds) = p.fd() {
                for fd in fds.into_iter().flatten() {
                    if let procfs::process::FDTarget::Socket(inode) = fd.target {
                        inode_pid.entry(inode).or_insert(pid);
                    }
                }
            }
        }
    }
    Ok(rows
        .into_iter()
        .map(|(address, port, inode)| TcpListener {
            address,
            port,
            pid: inode_pid.get(&inode).copied().unwrap_or(0),
        })
        .collect())
}

/// 解析 `/proc/net/tcp{,6}` 的 LISTEN 行 → (规范地址, 端口, inode)。
/// `st == 0A` 即 LISTEN；v4 地址是 LE u32 的 hex，v6 是 4 个 LE u32 的 hex。
#[cfg(target_os = "linux")]
fn parse_proc_net_tcp(text: &str) -> Vec<(String, u16, u64)> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 10 || cols[3] != "0A" {
            continue;
        }
        let Some((ip_hex, port_hex)) = cols[1].split_once(':') else {
            continue;
        };
        let Ok(port) = u16::from_str_radix(port_hex, 16) else {
            continue;
        };
        let address = if ip_hex.len() == 8 {
            let v = u32::from_str_radix(ip_hex, 16).unwrap_or(0);
            let b = v.to_le_bytes();
            format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3])
        } else if ip_hex.len() == 32 {
            let mut octets = [0u8; 16];
            for (i, seg) in ip_hex.as_bytes().chunks(8).enumerate() {
                let hex = std::str::from_utf8(seg).unwrap_or("0");
                let v = u32::from_str_radix(hex, 16).unwrap_or(0);
                octets[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
            }
            format!("[{}]", std::net::Ipv6Addr::from(octets))
        } else {
            continue;
        };
        let Ok(inode) = cols[9].parse::<u64>() else {
            continue;
        };
        out.push((address, port, inode));
    }
    out
}

#[cfg(target_os = "macos")]
fn macos_listeners() -> Result<Vec<TcpListener>> {
    let out = Command::new("lsof")
        .args(["-nP", "-iTCP", "-sTCP:LISTEN"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| {
            Error::new(
                ErrorCode::PortScanFailed,
                format!("无法读取端口表（lsof）: {e}"),
            )
        })?;
    if !out.status.success() {
        // lsof 约定：exit 1 = 无匹配。本机没有任何 TCP LISTEN（如 CI 虚拟机）是
        // 合法空表，不是故障；其余退出码才是真正的读取失败。
        if out.status.code() == Some(1) && out.stdout.is_empty() {
            return Ok(Vec::new());
        }
        return Err(Error::new(
            ErrorCode::PortScanFailed,
            "lsof 非零退出，端口表不可用",
        ));
    }
    Ok(parse_lsof_listeners(&String::from_utf8_lossy(&out.stdout)))
}

/// 解析 `lsof -nP -iTCP -sTCP:LISTEN` 输出（跳表头）：`PID` 第 2 列，
/// `NAME` 形如 `*:8081` / `127.0.0.1:8081` / `[::]:6379`。
/// 纯解析逻辑，`test` 下全平台编译以便单测覆盖。
#[cfg(any(target_os = "macos", test))]
fn parse_lsof_listeners(text: &str) -> Vec<TcpListener> {
    let mut out = Vec::new();
    for line in text.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 9 {
            continue;
        }
        let Ok(pid) = cols[1].parse::<u32>() else {
            continue;
        };
        let name = cols[8];
        let Some((addr, port)) = split_addr_port(name) else {
            continue;
        };
        // lsof 通配符 `*` 归一化为 0.0.0.0（与 Windows netstat 口径一致）
        let addr = if addr == "*" {
            "0.0.0.0".to_string()
        } else {
            addr
        };
        out.push(TcpListener {
            address: addr,
            port,
            pid,
        });
    }
    out
}

/// 服务自身运行态：排除「自己的占用」时使用。
/// - 托管实例：拿整个 Job 树 pid（含根进程，mvn 派生的 java 也会在内），
///   按这批 pid 精确排除——Spring 由 mvn 派生 java 实际监听，只拿根 pid 会漏；
/// - 打开时按端口识别的外部接管实例：无 Job 句柄、`pid` 未知但
///   `running=true`，该端口上的监听即自身实例，整体排除。
#[derive(Debug, Clone, Default)]
pub struct OwnRuntime {
    /// 本服务 Job 进程树全部存活 pid（含根）。空表示无可枚举的自身进程。
    pub pids: Vec<u32>,
    /// 无 Job 句柄但服务在跑（外部接管实例）：该端口上的监听整体视为自身。
    pub running: bool,
}

/// 该端口上「非自身」的监听者：剥离自身进程树/接管实例后的占用判据。
fn non_own<'a>(listeners: &'a [TcpListener], port: u16, own: &OwnRuntime) -> Vec<&'a TcpListener> {
    listeners
        .iter()
        .filter(|l| l.port == port)
        .filter(|l| {
            if own.pids.is_empty() {
                // 无自身进程树：接管实例 running 时端口整体归自己，否则照常判定
                !own.running
            } else {
                // 进程树内任何 pid（含派生子进程）都算自己
                !own.pids.contains(&l.pid)
            }
        })
        .collect()
}

/// §5.1：每个有 `port` 的服务一条。`managed` = listener PID 属于本引擎 Job。
/// 「排除自己」：listener 属于该服务自身运行实例（见 [`OwnRuntime`]）时不计
/// 占用——运行中的服务检查自己的端口是常态操作，不应提示「已被占用」。
pub fn inspect(
    spec: &SuperTaskFile,
    listeners: &[TcpListener],
    managed_pids: &std::collections::HashSet<u32>,
    own: &std::collections::HashMap<String, OwnRuntime>,
) -> Vec<crate::ipc::PortInspection> {
    let mut items = Vec::new();
    for (id, svc) in &spec.services {
        let Some(port) = svc.port else { continue };
        let self_run = own.get(id).cloned().unwrap_or_default();
        let hit = non_own(listeners, port, &self_run);
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

/// §5.1：检查单个「候选端口」是否可被 `id` 服务使用（用输入框里填的号，而非
/// 配置端口）。自身进程树与接管实例同样豁免——运行中的服务复查自己的端口不应
/// 报「已被占用」。`managed` = 监听者 pid 属于本引擎其它 Job 树。
pub fn inspect_single(
    spec: &SuperTaskFile,
    id: &str,
    port: u16,
    listeners: &[TcpListener],
    managed_pids: &std::collections::HashSet<u32>,
    own: &std::collections::HashMap<String, OwnRuntime>,
) -> Result<crate::ipc::PortInspection> {
    if !spec.services.contains_key(id) {
        return Err(Error::new(ErrorCode::NotFound, format!("没有服务 {id}")));
    }
    let self_run = own.get(id).cloned().unwrap_or_default();
    let hit = non_own(listeners, port, &self_run);
    let in_use = !hit.is_empty();
    let (pid, process, managed) = match hit.first() {
        Some(l) => (
            Some(l.pid),
            process_name_of(l.pid),
            managed_pids.contains(&l.pid),
        ),
        None => (None, None, false),
    };
    Ok(crate::ipc::PortInspection {
        id: id.to_string(),
        port,
        in_use,
        pid,
        process_name: process,
        managed,
    })
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
fn process_name_of(pid: u32) -> Option<String> {
    #[cfg(target_os = "linux")]
    {
        let text = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
        text.lines().next().map(str::to_string)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let out = Command::new("ps")
            .args(["-o", "comm=", "-p", &pid.to_string()])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines().next().map(|l| l.trim().to_string())
    }
}

/// §5.2：从当前端口向上扫，跳过其他服务的 port/ports、系统保留段与已发现监听；
/// 最多检查 128 个候选、返回最多 5 个。
pub fn suggest(spec: &SuperTaskFile, id: &str, listeners: &[TcpListener]) -> Result<Vec<u16>> {
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
pub fn apply_port_assign(spec: &mut SuperTaskFile, id: &str, new_port: u16) -> Result<Vec<String>> {
    let svc = spec
        .services
        .get_mut(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    let Some(old_port) = svc.port else {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            format!("{id} 没有配置 port"),
        ));
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
                notes.push(format!(
                    "显式环境变量 {key}={v} 未改（与端口不一致，保留原值）"
                ));
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
        // 1.7 §4.4：python/go 与 node 同口径；generic 无生态约定不注入
        "node" | "python" | "go" => Some("PORT"),
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
        assert_eq!(
            ls[0],
            TcpListener {
                address: "127.0.0.1".into(),
                port: 8081,
                pid: 4120
            }
        );
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
        // db 自己以 1234 监听 6379（托管运行中的常态）；api 未运行
        let mut own = std::collections::HashMap::new();
        own.insert(
            "db".to_string(),
            OwnRuntime {
                pids: vec![1234],
                running: true,
            },
        );
        own.insert("api".to_string(), OwnRuntime::default());
        let items = inspect(&s, &parse_netstat_listeners(NETSTAT), &managed, &own);
        assert_eq!(items.len(), 2);
        let api = items.iter().find(|i| i.id == "api").unwrap();
        assert!(api.in_use && !api.managed && api.pid == Some(4120));
        // process_name 走真实 tasklist，不按内容断言（本机进程表不可预测）
        // db 的 6379 只被自己（1234）监听 → 排除自己 → 不算占用
        let db = items.iter().find(|i| i.id == "db").unwrap();
        assert!(!db.in_use, "自身监听不计入占用");
        assert!(!db.managed && db.pid.is_none());
    }

    #[test]
    fn inspect_excludes_own_process_tree() {
        // 托管 Spring：根 mvn pid=2000，实际监听端口的是派生 java pid=2001。
        // 只排除根会把 java 判成「外部进程占用」→ 必须整个进程树一起排除。
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8081\n";
        let s = spec(y);
        let mut managed = std::collections::HashSet::new();
        managed.insert(2000u32);
        managed.insert(2001u32);
        let mut own = std::collections::HashMap::new();
        own.insert(
            "api".to_string(),
            OwnRuntime {
                pids: vec![2000, 2001],
                running: true,
            },
        );
        let listeners = vec![
            TcpListener {
                address: "127.0.0.1".into(),
                port: 8081,
                pid: 2001,
            },
            TcpListener {
                address: "0.0.0.0".into(),
                port: 9000,
                pid: 4120,
            },
        ];
        let items = inspect(&s, &listeners, &managed, &own);
        let api = items.iter().find(|i| i.id == "api").unwrap();
        assert!(!api.in_use, "派生 java 的监听同样排除，不报外部占用");
        assert!(api.pid.is_none());
    }

    #[test]
    fn inspect_single_checks_candidate_port_and_excludes_own_tree() {
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8081\n";
        let s = spec(y);
        let mut managed = std::collections::HashSet::new();
        managed.insert(2000u32);
        managed.insert(2001u32);
        let mut own = std::collections::HashMap::new();
        own.insert(
            "api".to_string(),
            OwnRuntime {
                pids: vec![2000, 2001],
                running: true,
            },
        );
        let listeners = vec![
            TcpListener {
                address: "127.0.0.1".into(),
                port: 8081,
                pid: 2001,
            },
            TcpListener {
                address: "0.0.0.0".into(),
                port: 9000,
                pid: 4120,
            },
        ];
        // 候选端口 = 自身当前监听端口（java 持有）→ 豁免，不算占用
        let own_port = inspect_single(&s, "api", 8081, &listeners, &managed, &own).unwrap();
        assert!(!own_port.in_use && own_port.pid.is_none());
        // 候选端口 9000 被外部进程持有 → 占用、非托管
        let external = inspect_single(&s, "api", 9000, &listeners, &managed, &own).unwrap();
        assert!(external.in_use && !external.managed && external.pid == Some(4120));
    }

    #[test]
    fn inspect_excludes_external_adopted_instance() {
        // 打开时按端口识别的外部实例（PID 未知、running=true）：
        // 该端口上的监听就是自己 → 不算占用
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8081\n";
        let s = spec(y);
        let mut own = std::collections::HashMap::new();
        own.insert(
            "api".to_string(),
            OwnRuntime {
                pids: vec![],
                running: true,
            },
        );
        let items = inspect(
            &s,
            &parse_netstat_listeners(NETSTAT),
            &std::collections::HashSet::new(),
            &own,
        );
        let api = items.iter().find(|i| i.id == "api").unwrap();
        assert!(!api.in_use, "外部接管实例的自身监听同样排除");
    }

    #[test]
    fn inspect_reports_other_managed_holder() {
        // 其他服务的托管进程占用了本服务端口 → 仍算占用，并标记 managed
        let y = "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 9000\n";
        let s = spec(y);
        let mut managed = std::collections::HashSet::new();
        managed.insert(1234u32);
        let mut own = std::collections::HashMap::new();
        own.insert(
            "api".to_string(),
            OwnRuntime {
                pids: vec![7],
                running: true,
            },
        );
        let listeners = vec![TcpListener {
            address: "0.0.0.0".into(),
            port: 9000,
            pid: 1234,
        }];
        let items = inspect(&s, &listeners, &managed, &own);
        let api = items.iter().find(|i| i.id == "api").unwrap();
        assert!(api.in_use && api.managed && api.pid == Some(1234));
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

    // ---- 1.4 Unix 端口表解析（三平台 CI 跑；本机仅对应平台执行）----

    #[cfg(target_os = "linux")]
    #[test]
    fn proc_net_tcp_parses_listen_v4_v6() {
        // 0100007F:1F91 = 127.0.0.1:8081（LE）；:1F90 = 8080（LE hex）；0A = LISTEN
        let text = "  sl local_address rem_address   st tx:rx:tr:when retrnsmt uid timeout inode\n   0: 0100007F:1F91 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 12345 1\n   1: 00000000:1F90 00000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 23456 1\n   2: 00000000:1F91 00000000:0000 01 00000000:00000000 00:00000000 00000000     0        0 34567 1\n";
        let rows = parse_proc_net_tcp(text);
        assert_eq!(rows.len(), 2, "只解析 LISTEN(0A) 行");
        assert_eq!(rows[0], ("127.0.0.1".to_string(), 8081u16, 12345u64));
        assert_eq!(rows[1], ("0.0.0.0".to_string(), 8080u16, 23456u64));

        let v6 = "  sl local_address rem_address   st tx:rx:tr:when retrnsmt uid timeout inode\n   0: 00000000000000000000000000000000:1F91 00000000000000000000000000000000:0000 0A 00000000:00000000 00:00000000 00000000     0        0 45678 1\n";
        let rows6 = parse_proc_net_tcp(v6);
        assert_eq!(rows6.len(), 1);
        assert_eq!(rows6[0].0, "[::]");
        assert_eq!(rows6[0].1, 8081);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn lsof_parses_listen_rows() {
        let text = "COMMAND   PID  USER   FD   TYPE             DEVICE SIZE/OFF NODE NAME\njava    4120  user   45u  IPv6  0xabcdef      0t0  TCP *:8081 (LISTEN)\nnode    1234  user   23u  IPv4  0x123456      0t0  TCP 127.0.0.1:5173 (LISTEN)\n";
        let ls = parse_lsof_listeners(text);
        assert_eq!(ls.len(), 2);
        assert_eq!(ls[0].pid, 4120);
        assert_eq!(ls[0].address, "0.0.0.0", "lsof 通配符 * 归一化");
        assert_eq!(ls[0].port, 8081);
        assert_eq!(ls[1].pid, 1234);
        assert_eq!(ls[1].address, "127.0.0.1");
        assert_eq!(ls[1].port, 5173);
    }
}
