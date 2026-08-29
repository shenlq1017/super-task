//! 1.6 网关：路由模型 → 配置渲染 → 本机校验（规格 §4–§6）。
//!
//! 模块结构：`model`（IR）、`render`（三家配置纯函数渲染）、`probe`（二进制
//! 探测）、`validate`（spawn `nginx -t` / `caddy validate` / `httpd -t`）。
//! 引擎托管（GatewaySlot）在 `engine.rs`。

pub mod model;
pub mod probe;
pub mod render;
pub mod validate;

pub use model::{resolve, parse_upstream, GatewayLocation, GatewayServerGroup, ResolvedGateway, UpstreamAddr, MAX_ROUTES};

use crate::error::{Error, ErrorCode, Result};
use crate::spec::{SuperTaskFile, GatewayConf};

/// 一条静态校验问题（§4.1）：`route` 为路由序号（0 基），全局问题为 None。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GatewayIssue {
    pub route: Option<usize>,
    pub message: String,
}

impl GatewayIssue {
    fn new(route: Option<usize>, message: impl Into<String>) -> Self {
        Self {
            route,
            message: message.into(),
        }
    }
}

/// 路由静态校验（纯逻辑，无 IO）。打开工作区 → warning；apply/start → 硬错误
/// （`ensure_static`）。未配置（无 kind）不产生问题——那是 GATEWAY_NOT_CONFIGURED
/// 的触发条件，不是配置错误。
pub fn validate_static(file: &SuperTaskFile, conf: &GatewayConf) -> Vec<GatewayIssue> {
    let mut issues = Vec::new();
    if conf.kind.is_none() {
        // 未配置段（含 1.0 reserved `gateway: {}`）：零问题零行为
        return issues;
    }
    if !(1024..=65535).contains(&conf.port) {
        issues.push(GatewayIssue::new(
            None,
            format!("gateway.port {} 超出 1024–65535", conf.port),
        ));
    }
    // gateway.port 不得与任一服务 port 相同（§7）
    for (id, svc) in &file.services {
        let clash = svc.port == Some(conf.port) || svc.ports.contains(&conf.port);
        if clash {
            issues.push(GatewayIssue::new(
                None,
                format!("gateway.port {} 与服务 {id:?} 的端口重复", conf.port),
            ));
        }
    }
    if conf.routes.len() > MAX_ROUTES {
        issues.push(GatewayIssue::new(
            None,
            format!("路由数超过 {MAX_ROUTES}"),
        ));
    }
    let mut seen: Vec<(String, String)> = Vec::new();
    for (i, route) in conf.routes.iter().enumerate() {
        let host = normalized_host(route);
        if let Some(h) = &host {
            if !is_valid_hostname(h) {
                issues.push(GatewayIssue::new(
                    Some(i),
                    format!("host {h:?} 非法（只允许 hostname 或 *.localhost 形式子域）"),
                ));
            }
        }
        if !route.path.starts_with('/') || route.path.contains(char::is_whitespace) {
            issues.push(GatewayIssue::new(
                Some(i),
                format!("path {:?} 非法：必须以 / 开头的路径前缀", route.path),
            ));
        }
        let key_host = host.unwrap_or_default();
        if seen.iter().any(|(h, p)| *h == key_host && *p == route.path) {
            issues.push(GatewayIssue::new(
                Some(i),
                format!(
                    "路由 (host={:?}, path={:?}) 重复",
                    if key_host.is_empty() { None } else { Some(key_host.clone()) },
                    route.path
                ),
            ));
        } else {
            seen.push((key_host, route.path.clone()));
        }
        let has_target = route.target.as_deref().map(str::trim).unwrap_or("").len() > 0;
        let has_upstream = route.upstream.as_deref().map(str::trim).unwrap_or("").len() > 0;
        if has_target == has_upstream {
            issues.push(GatewayIssue::new(
                Some(i),
                "target 与 upstream 必须二选一",
            ));
            continue;
        }
        if has_target {
            let target = route.target.as_deref().unwrap_or("").trim();
            match file.services.get(target) {
                None => issues.push(GatewayIssue::new(
                    Some(i),
                    format!("target 服务 {target:?} 不存在"),
                )),
                Some(svc) => {
                    if svc.port.is_none() && svc.ports.is_empty() {
                        issues.push(GatewayIssue::new(
                            Some(i),
                            format!("target 服务 {target:?} 没有配置 port，无法解析上游地址"),
                        ));
                    }
                }
            }
        }
        if has_upstream {
            if let Err(e) = model::parse_upstream(route.upstream.as_deref().unwrap_or("")) {
                issues.push(GatewayIssue::new(Some(i), e.message().to_string()));
            }
        }
    }
    issues
}

/// 硬错误路径（apply / start 前置）：有静态问题 → GATEWAY_ROUTE_INVALID，
/// details 带全部问题列表。
pub fn ensure_static(file: &SuperTaskFile, conf: &GatewayConf) -> Result<()> {
    let issues = validate_static(file, conf);
    if issues.is_empty() {
        return Ok(());
    }
    let messages: Vec<String> = issues
        .iter()
        .map(|i| match i.route {
            Some(idx) => format!("第 {} 条路由：{}", idx + 1, i.message),
            None => i.message.clone(),
        })
        .collect();
    Err(Error::new(
        ErrorCode::GatewayRouteInvalid,
        format!("网关路由校验失败：{}", messages.join("；")),
    )
    .details(serde_yaml::to_value(&messages).unwrap_or(serde_yaml::Value::Null)))
}

fn normalized_host(route: &crate::spec::GatewayRoute) -> Option<String> {
    route
        .host
        .as_deref()
        .map(str::trim)
        .filter(|h| !h.is_empty())
        .map(str::to_string)
}

/// hostname 规则：字母数字/连字符标签；`*.` 通配只允许出现在首位（
/// `*.localhost` 子域形式）；无 scheme / 端口 / 路径。
pub fn is_valid_hostname(h: &str) -> bool {
    if h.is_empty() || h.len() > 253 || h.chars().any(char::is_whitespace) {
        return false;
    }
    if h.contains(':') || h.contains('/') || h.contains('\\') || h.contains('@') {
        return false;
    }
    let h = h.strip_suffix('.').unwrap_or(h);
    let labels: Vec<&str> = h.split('.').collect();
    if labels.iter().any(|l| l.is_empty()) {
        return false;
    }
    for (i, label) in labels.iter().enumerate() {
        if i == 0 && *label == "*" {
            continue;
        }
        if label.len() > 63 {
            return false;
        }
        if !label
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        {
            return false;
        }
        if label.starts_with('-') || label.ends_with('-') {
            return false;
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;

    fn ws(yaml: &str) -> SuperTaskFile {
        parse_yaml(yaml).unwrap().0
    }

    fn conf_of(file: &SuperTaskFile) -> GatewayConf {
        file.gateway.clone().unwrap()
    }

    const BASE: &str = "version: 1\nservices:\n  user-api:\n    kind: spring-boot\n    module: api\n    port: 8081\n  web:\n    kind: node\n    dir: web\n    port: 5173\n";

    #[test]
    fn gateway_empty_section_round_trips_and_is_unconfigured() {
        let text = format!("{BASE}gateway: {{}}\n");
        let (f, _) = parse_yaml(&text).unwrap();
        let conf = f.gateway.as_ref().unwrap();
        assert_eq!(conf.kind, None);
        assert!(conf.enabled);
        assert_eq!(conf.port, 8080);
        assert!(validate_static(&f, conf).is_empty(), "未配置段零问题");
        // round-trip 后仍是空映射（缺省字段跳过）
        let out = crate::spec::to_yaml(&f).unwrap();
        let (f2, _) = parse_yaml(&out).unwrap();
        assert_eq!(f2.gateway.as_ref().unwrap().kind, None);
        assert!(out.contains("gateway:"));
    }

    #[test]
    fn gateway_unknown_fields_round_trip() {
        let text = format!("{BASE}gateway:\n  kind: nginx\n  x-future: keep-me\n");
        let (f, _) = parse_yaml(&text).unwrap();
        let conf = f.gateway.as_ref().unwrap();
        assert!(conf.extra.contains_key("x-future"));
        let out = crate::spec::to_yaml(&f).unwrap();
        let (f2, _) = parse_yaml(&out).unwrap();
        assert!(f2.gateway.as_ref().unwrap().extra.contains_key("x-future"));
    }

    #[test]
    fn static_issues_each_rule() {
        // port 越界
        let text = format!("{BASE}gateway:\n  kind: nginx\n  port: 80\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.route.is_none() && i.message.contains("1024")));

        // 与服务端口冲突
        let text = format!("{BASE}gateway:\n  kind: nginx\n  port: 8081\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("8081")));

        // (host, path) 重复
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /api\n      target: user-api\n    - path: /api\n      target: web\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("重复")));

        // path 非法
        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: api\n      target: user-api\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("path")));

        // target 不存在 / 无端口 / 互斥缺失 / 双填
        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      target: ghost\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("ghost")));

        let text = format!(
            "{BASE}  noport:\n    kind: node\n    dir: np\n    health:\n      type: none\n\ngateway:\n  kind: nginx\n  routes:\n    - path: /x\n      target: noport\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("没有配置 port")));

        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /x\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("二选一")));

        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /x\n      target: user-api\n      upstream: 127.0.0.1:9\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("二选一")));

        // upstream 语法
        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /x\n      upstream: http://127.0.0.1:9\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("upstream")));

        // host 非法
        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - host: \"bad_host!!\"\n      path: /\n      target: user-api\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("host")));
    }

    #[test]
    fn ensure_static_details_carry_messages() {
        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      target: ghost\n");
        let file = ws(&text);
        let conf = conf_of(&file);
        let e = ensure_static(&file, &conf).unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayRouteInvalid);
        assert!(e.message().contains("ghost"));
    }

    #[test]
    fn valid_localhost_subdomains_pass() {
        assert!(is_valid_hostname("api.localhost"));
        assert!(is_valid_hostname("*.localhost"));
        assert!(is_valid_hostname("127.0.0.1"));
        assert!(is_valid_hostname("a-b.example.com"));
        assert!(!is_valid_hostname("-lead"));
        assert!(!is_valid_hostname("trail-"));
        assert!(!is_valid_hostname("a..b"));
        assert!(!is_valid_hostname("*x.localhost"));
        assert!(!is_valid_hostname("a*b.localhost"));
        assert!(!is_valid_hostname("host:80"));
    }
}
