//! 1.6 网关中间表示（IR）：渲染前的引擎无关模型。
//!
//! `resolve` 把 typed `GatewayConf` 解析为 [`ResolvedGateway`]：target 服务 id
//! 解析为 upstream 地址（端口来自当前 yaml；v4/v6 回环地址选择由引擎调用侧
//! 通过 `host_for_port` 闭包注入，本模块保持纯函数）。

use crate::error::{Error, ErrorCode, Result};
use crate::spec::{GatewayConf, GatewayKind, GatewayRoute, GatewayTls, SuperTaskFile};

/// 路由数上限（§1.2 路由模型；防失控配置）。
pub const MAX_ROUTES: usize = 64;

/// 渲染就绪的上游地址：host 已含 IPv6 括号（如 `127.0.0.1` / `[::1]`）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamAddr {
    pub host: String,
    pub port: u16,
}

impl UpstreamAddr {
    /// `http://` 代理目标文本（nginx proxy_pass / caddy reverse_proxy 同形）。
    pub fn url(&self) -> String {
        format!("http://{}:{}", self.host, self.port)
    }
}

/// host 分组下的一个 location：path 前缀 + 上游。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayLocation {
    pub path: String,
    pub upstream: UpstreamAddr,
}

/// host 分组：None = 全匹配（catch-all，nginx default_server / caddy 根站点）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayServerGroup {
    pub host: Option<String>,
    pub locations: Vec<GatewayLocation>,
}

/// 渲染输入 IR。不含引擎类型分支——平台差异只在 argv/探测，不在配置内容。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedGateway {
    pub kind: GatewayKind,
    pub port: u16,
    pub tls: GatewayTls,
    /// 按首次出现顺序的 host 分组；组内 location 保留声明顺序
    /// （渲染时按最长前缀排序）。
    pub groups: Vec<GatewayServerGroup>,
    /// apache LoadModule 目录前缀（引擎侧由 bin 位置注入；`Caddyfile`/nginx
    /// 渲染忽略）。纯函数纪律：平台路径作为入参，不内置探测。
    pub apache_modules_dir: Option<String>,
}

/// 显式 `upstream` 语法校验 + 解析（§9.2：拒绝 URL、userinfo、scheme、空白）。
/// 接受 `127.0.0.1:9000` / `[::1]:9000` / `localhost:9000`。
pub fn parse_upstream(raw: &str) -> Result<UpstreamAddr> {
    let bad = |why: &str| {
        Err(Error::new(
            ErrorCode::GatewayRouteInvalid,
            format!("upstream {raw:?} 非法（{why}）：应为 host:port"),
        ))
    };
    let s = raw.trim();
    if s.is_empty() {
        return bad("为空");
    }
    if s.chars().any(char::is_whitespace) {
        return bad("含空白");
    }
    if s.contains("://") {
        return bad("不允许 scheme");
    }
    if s.contains('@') {
        return bad("不允许 userinfo");
    }
    if s.contains('/') || s.contains('\\') || s.contains('?') || s.contains('#') {
        return bad("不允许路径");
    }
    // host:port（IPv6 必须已带 []）
    let (host, port_str) = if let Some(rest) = s.strip_prefix('[') {
        let (h, p) = rest.split_once("]:").ok_or_else(|| {
            Error::new(
                ErrorCode::GatewayRouteInvalid,
                format!("upstream {raw:?} 非法：IPv6 地址应写作 [::1]:port"),
            )
        })?;
        (format!("[{h}]"), p)
    } else {
        match s.rsplit_once(':') {
            Some((h, p)) => (h.to_string(), p),
            None => return bad("缺少 :port"),
        }
    };
    if host.is_empty() {
        return bad("缺少 host");
    }
    let Ok(port) = port_str.parse::<u16>() else {
        return bad("port 不是 1–65535 数字");
    };
    if port == 0 {
        return bad("port 不能为 0");
    }
    Ok(UpstreamAddr { host, port })
}

/// 服务端口解析：`port` 优先，其次 `ports` 首个。
fn service_port(svc: &crate::spec::ServiceSpec) -> Option<u16> {
    svc.port.or_else(|| svc.ports.first().copied())
}

/// 把 GatewayConf 解析为 IR。`host_for_port`：端口 → 回环 host 文本
/// （引擎侧按 1.2 监听表注入 v4/v6 选择；测试用恒等 `127.0.0.1`）。
/// 前置条件：静态校验已通过（target 存在等在 `validate_static` 负责，
/// 这里对缺失 target 返回 GATEWAY_ROUTE_INVALID 兜底）。
pub fn resolve(
    file: &SuperTaskFile,
    conf: &GatewayConf,
    host_for_port: &dyn Fn(u16) -> String,
) -> Result<ResolvedGateway> {
    let kind = conf.kind.ok_or_else(|| {
        Error::new(ErrorCode::GatewayNotConfigured, "gateway 段未配置 kind")
    })?;
    let mut groups: Vec<GatewayServerGroup> = Vec::new();
    for (i, route) in conf.routes.iter().enumerate() {
        let upstream = resolve_upstream_of(file, route, i)?;
        let upstream = UpstreamAddr {
            host: host_for_port(upstream.port),
            port: upstream.port,
        };
        let host = normalized_host(route);
        if let Some(group) = groups.iter_mut().find(|g| g.host == host) {
            group.locations.push(GatewayLocation {
                path: route.path.clone(),
                upstream,
            });
        } else {
            groups.push(GatewayServerGroup {
                host,
                locations: vec![GatewayLocation {
                    path: route.path.clone(),
                    upstream,
                }],
            });
        }
    }
    Ok(ResolvedGateway {
        kind,
        port: conf.port,
        tls: conf.tls,
        groups,
        apache_modules_dir: None,
    })
}

fn resolve_upstream_of(file: &SuperTaskFile, route: &GatewayRoute, index: usize) -> Result<UpstreamAddr> {
    if let Some(up) = &route.upstream {
        return parse_upstream(up);
    }
    let Some(target) = route.target.as_deref() else {
        return Err(route_invalid(index, "target 与 upstream 必须二选一"));
    };
    let svc = file
        .services
        .get(target)
        .ok_or_else(|| route_invalid(index, format!("target 服务 {target:?} 不存在")))?;
    let port = service_port(svc)
        .ok_or_else(|| route_invalid(index, format!("target 服务 {target:?} 没有配置 port")))?;
    Ok(UpstreamAddr {
        host: String::new(), // host 由 host_for_port 注入（resolve 统一替换）
        port,
    })
}

pub(crate) fn route_invalid(index: usize, why: impl Into<String>) -> Error {
    Error::new(
        ErrorCode::GatewayRouteInvalid,
        format!("第 {} 条路由：{}", index + 1, why.into()),
    )
}

/// host 归一化：None / 空 → None（catch-all）。
fn normalized_host(route: &GatewayRoute) -> Option<String> {
    route
        .host
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;

    fn ws(services: &str, gateway: &str) -> SuperTaskFile {
        let text = format!("version: 1\nservices:\n{services}\ngateway:\n{gateway}");
        parse_yaml(&text).unwrap().0
    }

    const SERVICES: &str = "  user-api:\n    kind: spring-boot\n    module: api\n    port: 8081\n  web:\n    kind: node\n    dir: web\n    port: 5173\n";

    #[test]
    fn parse_upstream_accepts_host_port_and_ipv6() {
        let u = parse_upstream("127.0.0.1:9000").unwrap();
        assert_eq!(u.host, "127.0.0.1");
        assert_eq!(u.port, 9000);
        let u6 = parse_upstream("[::1]:9000").unwrap();
        assert_eq!(u6.host, "[::1]");
        assert_eq!(u6.url(), "http://[::1]:9000");
    }

    #[test]
    fn parse_upstream_rejects_url_userinfo_scheme() {
        for bad in [
            "http://127.0.0.1:9000",
            "user:pass@127.0.0.1:9000",
            "127.0.0.1:9000/api",
            "127.0.0.1",
            "127.0.0.1:notaport",
            "127.0.0.1:0",
            " 127.0.0.1:1 x",
        ] {
            assert!(parse_upstream(bad).is_err(), "{bad} 应被拒绝");
        }
    }

    #[test]
    fn resolve_groups_by_host_and_keeps_order() {
        let file = ws(
            SERVICES,
            "  kind: nginx\n  port: 8080\n  routes:\n    - path: /api\n      target: user-api\n    - host: api.localhost\n      path: /\n      target: user-api\n    - path: /\n      target: web\n",
        );
        let conf = file.gateway.clone().unwrap();
        let ir = resolve(&file, &conf, &|_| "127.0.0.1".into()).unwrap();
        assert_eq!(ir.kind, GatewayKind::Nginx);
        assert_eq!(ir.port, 8080);
        assert_eq!(ir.groups.len(), 2);
        // 空 host 组在先（首次出现），含 /api 与 / 两条
        assert_eq!(ir.groups[0].host, None);
        assert_eq!(ir.groups[0].locations.len(), 2);
        assert_eq!(ir.groups[0].locations[0].path, "/api");
        assert_eq!(ir.groups[0].locations[0].upstream.port, 8081);
        assert_eq!(ir.groups[1].host.as_deref(), Some("api.localhost"));
    }

    #[test]
    fn resolve_missing_target_fails() {
        let file = ws(SERVICES, "  kind: nginx\n  routes:\n    - path: /\n      target: ghost\n");
        let conf = file.gateway.clone().unwrap();
        let e = resolve(&file, &conf, &|_| "127.0.0.1".into()).unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayRouteInvalid);
        assert!(e.message().contains("ghost"));
    }

    #[test]
    fn resolve_explicit_upstream_wins() {
        let file = ws(SERVICES, "  kind: caddy\n  routes:\n    - path: /x\n      upstream: 127.0.0.1:9000\n");
        let conf = file.gateway.clone().unwrap();
        let ir = resolve(&file, &conf, &|_| "127.0.0.1".into()).unwrap();
        assert_eq!(ir.groups[0].locations[0].upstream.port, 9000);
    }

    #[test]
    fn resolve_without_kind_fails() {
        let file = ws(SERVICES, "  enabled: false\n");
        let conf = file.gateway.clone().unwrap();
        assert_eq!(
            resolve(&file, &conf, &|_| "127.0.0.1".into())
                .unwrap_err()
                .code(),
            ErrorCode::GatewayNotConfigured
        );
    }
}
