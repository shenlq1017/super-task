use std::io::{Read, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr, TcpStream};
use std::time::Duration;

use crate::discover::ListenEndpoint;
use crate::spec::{HealthSpec, HealthType};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HealthResult {
    pub ok: bool,
    pub detail: String,
}

/// 默认探测：无进程树监听信息时走双栈回退（127.0.0.1 → [::1]）。
pub fn check(spec: &HealthSpec, port: Option<u16>) -> HealthResult {
    check_with_endpoints(spec, port, &[])
}

/// 端点感知探测。
///
/// TCP 目标优先级（高→低）：
/// 1. 进程树监听里存在「配置端口」→ 只探该端口的双栈地址；
/// 2. 进程树有任意 LISTEN → 探全部真实端点（IPv4 优先；根治扫描猜测端口错位）；
/// 3. 都没有 → 配置端口的 127.0.0.1 / [::1] 双栈回退。
///
/// 任一候选连通即健康；失败 detail 记录最后一个错误供 UI 展示。
pub fn check_with_endpoints(
    spec: &HealthSpec,
    port: Option<u16>,
    eps: &[ListenEndpoint],
) -> HealthResult {
    let timeout = Duration::from_secs(spec.timeout_secs.max(1) as u64);
    match spec.r#type {
        HealthType::None => HealthResult {
            ok: true,
            detail: "none".into(),
        },
        HealthType::Tcp => {
            let Some(addrs) = tcp_targets(port, eps) else {
                return HealthResult {
                    ok: false,
                    detail: "tcp 无可用目标（缺 port 且进程树未发现监听）".into(),
                };
            };
            connect_first(&addrs, timeout)
        }
        HealthType::Http => {
            let url = spec
                .http
                .clone()
                .or_else(|| port.map(|p| format!("http://127.0.0.1:{p}/actuator/health")));
            let Some(url) = url else {
                return HealthResult {
                    ok: false,
                    detail: "http 缺少 URL".into(),
                };
            };
            http_probe(&url, eps, timeout)
        }
    }
}

fn tcp_targets(port: Option<u16>, eps: &[ListenEndpoint]) -> Option<Vec<SocketAddr>> {
    if let Some(p) = port {
        let matched: Vec<SocketAddr> = eps
            .iter()
            .filter(|e| e.port == p)
            .map(|e| SocketAddr::new(e.ip, p))
            .collect();
        if !matched.is_empty() {
            return Some(matched);
        }
    }
    if !eps.is_empty() {
        // 引擎侧发现结果已按 IPv4 优先排序，这里保持传入顺序
        return Some(eps.iter().map(|e| SocketAddr::new(e.ip, e.port)).collect());
    }
    port.map(dual_stack)
}

pub fn tcp(host: &str, port: u16, timeout: Duration) -> HealthResult {
    match resolve_loopback(host, port) {
        Some(addr) => connect_first(std::slice::from_ref(&addr), timeout),
        None => HealthResult {
            ok: false,
            detail: "非 loopback".into(),
        },
    }
}

/// 依次尝试候选地址，任一连通即成功；detail 报告实际命中的端点。
fn connect_first(addrs: &[SocketAddr], timeout: Duration) -> HealthResult {
    let mut last_err = String::from("无候选地址");
    for addr in addrs {
        match TcpStream::connect_timeout(addr, timeout) {
            Ok(_) => {
                return HealthResult {
                    ok: true,
                    detail: format!("tcp {addr} open"),
                };
            }
            Err(e) => last_err = format!("{addr}: {e}"),
        }
    }
    HealthResult {
        ok: false,
        detail: last_err,
    }
}

pub fn http(url: &str, timeout: Duration) -> HealthResult {
    http_probe(url, &[], timeout)
}

/// HTTP 候选：URL 同端口的真实监听 + host 的 loopback 双栈展开，逐个尝试（2xx 即健康）。
fn http_probe(url: &str, eps: &[ListenEndpoint], timeout: Duration) -> HealthResult {
    let Some((host, port, path)) = split_http_url(url) else {
        return HealthResult {
            ok: false,
            detail: "非法 URL".into(),
        };
    };

    let mut addrs: Vec<SocketAddr> = Vec::new();
    for e in eps.iter().filter(|e| e.port == port) {
        push_addr(&mut addrs, SocketAddr::new(e.ip, e.port));
    }
    // host 展开：localhost 双栈；字面量只取对应族
    let hosts: &[IpAddr] = match host.as_str() {
        "::1" | "[::1]" => &[IpAddr::V6(Ipv6Addr::LOCALHOST)],
        _ => &[
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            IpAddr::V6(Ipv6Addr::LOCALHOST),
        ],
    };
    for ip in hosts {
        push_addr(&mut addrs, SocketAddr::new(*ip, port));
    }

    let mut last_err = String::from("无候选地址");
    for addr in &addrs {
        match http_via(*addr, &host, port, &path, timeout) {
            Ok(r) => return r,
            Err(e) => last_err = e,
        }
    }
    HealthResult {
        ok: false,
        detail: last_err,
    }
}

fn push_addr(addrs: &mut Vec<SocketAddr>, addr: SocketAddr) {
    if !addrs.contains(&addr) {
        addrs.push(addr);
    }
}

fn http_via(
    addr: SocketAddr,
    host: &str,
    port: u16,
    path: &str,
    timeout: Duration,
) -> Result<HealthResult, String> {
    let mut stream =
        TcpStream::connect_timeout(&addr, timeout).map_err(|e| format!("{addr}: {e}"))?;
    let _ = stream.set_read_timeout(Some(timeout));
    let _ = stream.set_write_timeout(Some(timeout));
    let req = format!("GET {path} HTTP/1.0\r\nHost: {host}:{port}\r\nConnection: close\r\n\r\n");
    if let Err(e) = stream.write_all(req.as_bytes()) {
        return Err(format!("write {addr}: {e}"));
    }
    let mut buf = Vec::new();
    let mut tmp = [0u8; 512];
    while buf.len() < 2048 {
        match stream.read(&mut tmp) {
            Ok(0) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
            Err(_) => break,
        }
        if buf.windows(4).any(|w| w == b"\r\n\r\n") {
            break;
        }
    }
    let head = String::from_utf8_lossy(&buf);
    let status = head
        .lines()
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse::<u16>().ok());
    match status {
        Some(c) if (200..300).contains(&c) => Ok(HealthResult {
            ok: true,
            detail: format!("http {addr} {c}"),
        }),
        Some(c) => Err(format!("http {addr} {c}")),
        None => Err(format!("http {addr} 无状态行")),
    }
}

/// 配置端口双栈回退：127.0.0.1 优先、[::1] 兜底（Node/Vite 常默认只绑 IPv6）。
fn dual_stack(port: u16) -> Vec<SocketAddr> {
    vec![
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port),
        SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port),
    ]
}

fn resolve_loopback(host: &str, port: u16) -> Option<SocketAddr> {
    match host {
        "127.0.0.1" | "localhost" => Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), port)),
        "::1" | "[::1]" => Some(SocketAddr::new(IpAddr::V6(Ipv6Addr::LOCALHOST), port)),
        _ => None,
    }
}

fn split_http_url(url: &str) -> Option<(String, u16, String)> {
    let rest = url.strip_prefix("http://")?;
    let (hostport, path) = match rest.split_once('/') {
        Some((h, p)) => (h, format!("/{p}")),
        None => (rest, "/".into()),
    };
    let hostport = hostport.split('@').next_back()?;
    if let Some(h) = hostport.strip_prefix('[') {
        let (h, r) = h.split_once(']')?;
        let port = r
            .strip_prefix(':')
            .and_then(|p| p.parse().ok())
            .unwrap_or(80);
        return Some((format!("[{h}]"), port, path));
    }
    let (host, port) = match hostport.split_once(':') {
        Some((h, p)) => (h.to_string(), p.parse().ok()?),
        None => (hostport.to_string(), 80),
    };
    Some((host, port, path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;
    use std::thread;

    fn accept_and_reply_200(listener: TcpListener) {
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = listener.accept() {
                let mut b = [0u8; 256];
                let _ = s.read(&mut b);
                let _ = s.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 0\r\n\r\n");
            }
        });
    }

    #[test]
    fn tcp_open_and_closed() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _ = listener.accept();
        });
        thread::sleep(Duration::from_millis(30));
        assert!(tcp("127.0.0.1", port, Duration::from_secs(1)).ok);
        assert!(!tcp("127.0.0.1", 1, Duration::from_millis(200)).ok);
    }

    /// 回归：Node/Vite 只监听 [::1] 时，无端点信息也要靠双栈回退探活
    #[test]
    fn ipv6_only_listener_alive_via_fallback() {
        let listener = std::net::TcpListener::bind("[::1]:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        // 持续 accept：本测试会连续两次连接（回退路径 + 端点路径），
        // 只 accept 一次线程返回后 socket 关闭会让第二次连接失败
        std::thread::spawn(move || loop {
            if listener.accept().is_err() {
                break;
            }
        });
        thread::sleep(Duration::from_millis(30));

        // 回退路径：check() 不带端点，IPv4 失败后落 [::1]
        let spec = HealthSpec {
            r#type: HealthType::Tcp,
            http: None,
            interval_secs: 2,
            timeout_secs: 2,
        };
        let r = check_with_endpoints(&spec, Some(port), &[]);
        assert!(r.ok, "双栈回退应命中 [::1]，detail={}", r.detail);
        assert!(
            r.detail.contains("[::1]"),
            "detail 应报告真实端点: {}",
            r.detail
        );

        // 端点感知路径：直接给定 [::1] 监听端点
        let eps = vec![ListenEndpoint {
            ip: IpAddr::V6(Ipv6Addr::LOCALHOST),
            port,
        }];
        let r2 = check_with_endpoints(&spec, Some(port), &eps);
        assert!(r2.ok);
    }

    /// 规则 1：配置端口在进程树里监听（哪怕全树另有其他监听），只探配置端口，
    /// 避免误把构建工具（如 esbuild）的随机端口当作服务存活依据
    #[test]
    fn cfg_port_wins_over_other_listeners() {
        let main = TcpListener::bind("127.0.0.1:0").unwrap();
        let noise = TcpListener::bind("127.0.0.1:0").unwrap();
        let main_port = main.local_addr().unwrap().port();
        let noise_port = noise.local_addr().unwrap().port();
        thread::spawn(move || {
            let _ = main.accept();
        });
        thread::spawn(move || {
            let _ = noise.accept();
        });
        thread::sleep(Duration::from_millis(30));

        let spec = HealthSpec {
            r#type: HealthType::Tcp,
            http: None,
            interval_secs: 2,
            timeout_secs: 2,
        };
        let eps = vec![
            ListenEndpoint {
                ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: main_port,
            },
            ListenEndpoint {
                ip: IpAddr::V4(Ipv4Addr::LOCALHOST),
                port: noise_port,
            },
        ];
        let r = check_with_endpoints(&spec, Some(main_port), &eps);
        assert!(
            r.ok && r.detail.contains(&main_port.to_string()),
            "{}",
            r.detail
        );

        // 规则 2：配置端口不在树上 → 探全部真实端点（端口对齐）
        let r2 = check_with_endpoints(&spec, Some(main_port.wrapping_add(10000)), &eps);
        assert!(r2.ok, "真实端点兜底应可连: {}", r2.detail);
    }

    #[test]
    fn http_2xx_vs_503() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        accept_and_reply_200(listener);
        thread::sleep(Duration::from_millis(30));
        let r = http(
            &format!("http://127.0.0.1:{port}/x"),
            Duration::from_secs(1),
        );
        assert!(r.ok, "{}", r.detail);
    }

    /// 回归：HTTP 探测对 [::1]-only 的服务要能通
    #[test]
    fn http_ipv6_only_listener() {
        let listener = TcpListener::bind("[::1]:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        accept_and_reply_200(listener);
        thread::sleep(Duration::from_millis(30));
        let r = http(
            &format!("http://localhost:{port}/health"),
            Duration::from_secs(1),
        );
        assert!(r.ok, "{}", r.detail);
        assert!(
            r.detail.contains("[::1]"),
            "detail 应记录命中地址: {}",
            r.detail
        );
    }

    #[test]
    fn no_target_without_port_or_eps() {
        let spec = HealthSpec {
            r#type: HealthType::Tcp,
            http: None,
            interval_secs: 2,
            timeout_secs: 2,
        };
        let r = check_with_endpoints(&spec, None, &[]);
        assert!(!r.ok);
        assert!(r.detail.contains("无可用目标"), "{}", r.detail);
    }
}
