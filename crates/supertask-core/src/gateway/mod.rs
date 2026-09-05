//! 1.6 网关：路由模型 → 配置渲染 → 本机校验（规格 §4–§6）。
//!
//! 模块结构：`model`（IR）、`render`（三家配置纯函数渲染）、`probe`（二进制
//! 探测）、`validate`（spawn `nginx -t` / `caddy validate` / `httpd -t`）。
//! 引擎托管（GatewaySlot）在 `engine.rs`。

pub mod model;
pub mod probe;
pub mod render;
pub mod validate;

pub use model::{
    parse_upstream, resolve, GatewayCors, GatewayLocation, GatewayRedirect, GatewayServerGroup,
    ResolvedGateway, UpstreamAddr, MAX_ROUTES,
};

use crate::error::{Error, ErrorCode, Result};
use crate::spec::{GatewayConf, SuperTaskFile};

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

/// CORS 允许方法/头的缺省集与 preflight 缓存缺省（§7.1；渲染与解析共用）。
pub const CORS_DEFAULT_METHODS: &[&str] = &["GET", "POST", "PUT", "PATCH", "DELETE", "OPTIONS"];
pub const CORS_DEFAULT_HEADERS: &[&str] = &["Origin", "Content-Type", "Accept", "Authorization"];
pub const CORS_DEFAULT_MAX_AGE_SECS: u32 = 600;
/// CORS max_age_secs 上限（30 天）。
pub const CORS_MAX_AGE_CAP: u32 = 2_592_000;
/// 重定向允许的状态码；缺省 302。
pub const REDIRECT_STATUSES: &[u16] = &[301, 302, 307, 308];
pub const REDIRECT_DEFAULT_STATUS: u16 = 302;

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
        issues.push(GatewayIssue::new(None, format!("路由数超过 {MAX_ROUTES}")));
    }
    let mut seen: Vec<(String, String)> = Vec::new();
    for (i, route) in conf.routes.iter().enumerate() {
        let hosts = normalized_hosts(route);
        for h in &hosts {
            if !is_valid_hostname(h) {
                issues.push(GatewayIssue::new(
                    Some(i),
                    format!("host {h:?} 非法（只允许 hostname 或 *.localhost 形式子域，多域名用逗号分隔）"),
                ));
            }
        }
        if !route.path.starts_with('/') || route.path.contains(char::is_whitespace) {
            issues.push(GatewayIssue::new(
                Some(i),
                format!("path {:?} 非法：必须以 / 开头的路径前缀", route.path),
            ));
        }
        let key_host = canonical_host_key(&hosts);
        if seen.iter().any(|(h, p)| *h == key_host && *p == route.path) {
            issues.push(GatewayIssue::new(
                Some(i),
                format!(
                    "路由 (host={:?}, path={:?}) 重复",
                    if key_host.is_empty() {
                        None
                    } else {
                        Some(key_host.clone())
                    },
                    route.path
                ),
            ));
        } else {
            seen.push((key_host, route.path.clone()));
        }
        let has_target = route.target.as_deref().map(str::trim).unwrap_or("").len() > 0;
        let has_upstream = route.upstream.as_deref().map(str::trim).unwrap_or("").len() > 0;
        let has_redirect = route.redirect.as_deref().map(str::trim).unwrap_or("").len() > 0;
        let has_static = route
            .static_dir
            .as_deref()
            .map(str::trim)
            .unwrap_or("")
            .len()
            > 0;
        // 三种形态恰选其一（代理 = target/upstream 之一）
        let mut forms = 0;
        if has_redirect {
            forms += 1;
        }
        if has_static {
            forms += 1;
        }
        if has_target || has_upstream {
            forms += 1;
        }
        if forms != 1 {
            issues.push(GatewayIssue::new(
                Some(i),
                "target/upstream（代理）、redirect（重定向）、static_dir（静态站点）必须恰选其一",
            ));
            continue;
        }
        if route.cors.is_some() && !has_target && !has_upstream {
            issues.push(GatewayIssue::new(
                Some(i),
                "cors 仅适用于代理路由（target/upstream）",
            ));
        }
        if route.strip_prefix == Some(true) && !has_target && !has_upstream {
            issues.push(GatewayIssue::new(
                Some(i),
                "strip_prefix 仅适用于代理路由（target/upstream）",
            ));
        }
        if has_redirect {
            validate_redirect(route, i, &mut issues);
        }
        if has_static {
            validate_static_dir(route, i, &mut issues);
        }
        if has_target || has_upstream {
            if has_target && has_upstream {
                issues.push(GatewayIssue::new(Some(i), "target 与 upstream 必须二选一"));
            } else if has_target {
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
            } else if let Err(e) = model::parse_upstream(route.upstream.as_deref().unwrap_or("")) {
                issues.push(GatewayIssue::new(Some(i), e.message().to_string()));
            }
            if let Some(cors) = &route.cors {
                validate_cors(cors, i, &mut issues);
            }
        }
    }
    issues
}

fn validate_redirect(route: &crate::spec::GatewayRoute, i: usize, issues: &mut Vec<GatewayIssue>) {
    let to = route.redirect.as_deref().unwrap_or("").trim();
    let ok_target = to.starts_with('/') || to.starts_with("http://") || to.starts_with("https://");
    if !ok_target || to.chars().any(char::is_whitespace) {
        issues.push(GatewayIssue::new(
            Some(i),
            format!(
                "redirect {to:?} 非法：必须以 / 开头的路径或 http(s):// 开头的 URL，且不含空白"
            ),
        ));
    }
    if let Some(s) = route.redirect_status {
        if !REDIRECT_STATUSES.contains(&s) {
            issues.push(GatewayIssue::new(
                Some(i),
                format!("redirect_status {s} 非法：只允许 301/302/307/308"),
            ));
        }
    }
}

fn validate_static_dir(
    route: &crate::spec::GatewayRoute,
    i: usize,
    issues: &mut Vec<GatewayIssue>,
) {
    if route.path != "/" {
        issues.push(GatewayIssue::new(
            Some(i),
            format!(
                "static_dir 路由的 path 必须为 /（整站静态），当前为 {:?}",
                route.path
            ),
        ));
    }
    let dir = route.static_dir.as_deref().unwrap_or("").trim();
    let bad = dir.is_empty()
        || dir.starts_with('/')
        || dir.starts_with('\\')
        || dir.chars().next().is_some_and(|c| c == ':')
        || dir[1..].starts_with(':')
        || dir.contains(':');
    if bad {
        issues.push(GatewayIssue::new(
            Some(i),
            format!("static_dir {dir:?} 非法：必须是工作区内的相对目录（如 dist 或 web/dist）"),
        ));
        return;
    }
    let segments: Vec<&str> = dir.split(['/', '\\']).map(str::trim).collect();
    if segments.iter().any(|s| s.is_empty() || *s == ".") {
        issues.push(GatewayIssue::new(
            Some(i),
            format!("static_dir {dir:?} 非法：目录段不能为空或 ."),
        ));
    }
    if segments.iter().any(|s| *s == "..") {
        issues.push(GatewayIssue::new(
            Some(i),
            format!("static_dir {dir:?} 非法：不允许 .. 越出工作区"),
        ));
    }
}

fn validate_cors(cors: &crate::spec::GatewayCorsSpec, i: usize, issues: &mut Vec<GatewayIssue>) {
    if cors.origins.is_empty() {
        issues.push(GatewayIssue::new(Some(i), "cors.origins 不能为空"));
        return;
    }
    let wildcard = cors.origins.iter().any(|o| o == "*");
    if wildcard && cors.origins.len() > 1 {
        issues.push(GatewayIssue::new(
            Some(i),
            "cors.origins 的 * 不能与其他 origin 混用",
        ));
    }
    if wildcard && cors.credentials == Some(true) {
        issues.push(GatewayIssue::new(
            Some(i),
            "cors.credentials=true 时 origins 不能为 *（浏览器规范禁止通配 + 凭据）",
        ));
    }
    for o in &cors.origins {
        if o != "*" && !is_valid_origin(o) {
            issues.push(GatewayIssue::new(
                Some(i),
                format!("cors origin {o:?} 非法：应为 http(s)://host[:port]"),
            ));
        }
    }
    for list in [&cors.methods, &cors.headers] {
        if let Some(items) = list {
            if items
                .iter()
                .any(|m| m.trim().is_empty() || m.chars().any(char::is_whitespace))
            {
                issues.push(GatewayIssue::new(
                    Some(i),
                    "cors.methods / cors.headers 的每项必须是非空且不含空白的 token",
                ));
                break;
            }
        }
    }
    if let Some(age) = cors.max_age_secs {
        if age > CORS_MAX_AGE_CAP {
            issues.push(GatewayIssue::new(
                Some(i),
                format!("cors.max_age_secs {age} 超过上限 {CORS_MAX_AGE_CAP}"),
            ));
        }
    }
}

/// origin 校验：`http(s)://host[:port]`，host 支持 IPv6 括号形式；无路径/凭据。
pub fn is_valid_origin(o: &str) -> bool {
    let Some((scheme, rest)) = o.split_once("://") else {
        return false;
    };
    if !matches!(scheme, "http" | "https") || rest.is_empty() {
        return false;
    }
    if rest.contains(['/', '\\', '?', '#', '@']) || rest.chars().any(char::is_whitespace) {
        return false;
    }
    let (host, port, bracketed) = if let Some(inner) = rest.strip_prefix('[') {
        let Some((h, tail)) = inner.split_once(']') else {
            return false;
        };
        // ] 后只允许空或 :port
        let port = match tail {
            "" => None,
            t if t.starts_with(':') => Some(&t[1..]),
            _ => return false,
        };
        (h, port, true)
    } else {
        match rest.rsplit_once(':') {
            Some((h, p)) => (h, Some(p), false),
            None => (rest, None, false),
        }
    };
    if host.is_empty() {
        return false;
    }
    // 括号内是 IPv6 字面量（十六进制与冒号），hostnamed 规则不适用
    let host_ok = if bracketed {
        host.chars().all(|c| c.is_ascii_hexdigit() || c == ':')
    } else {
        is_valid_hostname(host)
    };
    if !host_ok {
        return false;
    }
    if let Some(p) = port {
        if !matches!(p.parse::<u16>(), Ok(1..=65535)) {
            return false;
        }
    }
    true
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

/// host 多域名拆分：逗号分隔，逐段 trim，剔除空段；空列表 = 全匹配 catch-all。
fn normalized_hosts(route: &crate::spec::GatewayRoute) -> Vec<String> {
    route
        .host
        .as_deref()
        .map(|h| {
            h.split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// 分组/查重键：排序去重后以 ", " 连接（同一域名集合必得同一键，渲染顺序确定）。
fn canonical_host_key(hosts: &[String]) -> String {
    let mut sorted: Vec<&str> = hosts.iter().map(String::as_str).collect();
    sorted.sort_unstable();
    sorted.dedup();
    sorted.join(", ")
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
        assert!(issues
            .iter()
            .any(|i| i.route.is_none() && i.message.contains("1024")));

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
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: api\n      target: user-api\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("path")));

        // target 不存在 / 无端口 / 互斥缺失 / 双填
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      target: ghost\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("ghost")));

        let text = format!(
            "{BASE}  noport:\n    kind: node\n    dir: np\n    health:\n      type: none\n\ngateway:\n  kind: nginx\n  routes:\n    - path: /x\n      target: noport\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("没有配置 port")));

        let text = format!("{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /x\n");
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("恰选其一")));

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
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      target: ghost\n"
        );
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

    #[test]
    fn valid_origins_and_rejections() {
        assert!(is_valid_origin("http://localhost:3000"));
        assert!(is_valid_origin("https://app.example.com"));
        assert!(is_valid_origin("http://127.0.0.1"));
        assert!(is_valid_origin("https://[::1]:8443"));
        assert!(!is_valid_origin("ftp://a.com"));
        assert!(!is_valid_origin("http://a.com/path"));
        assert!(!is_valid_origin("http://a.com?q=1"));
        assert!(!is_valid_origin("a.com"));
        assert!(!is_valid_origin("http://"));
        assert!(!is_valid_origin("http://bad_host"));
        assert!(!is_valid_origin("http://a.com:0"));
        assert!(!is_valid_origin("http://[::1]x"));
    }

    #[test]
    fn multi_host_and_new_route_forms_validate() {
        // 多域名合法
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - host: \"api.localhost, admin.localhost\"\n      path: /\n      target: user-api\n"
        );
        assert!(validate_static(&ws(&text), &conf_of(&ws(&text))).is_empty());

        // host 含非法段
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - host: \"api.localhost, bad host\"\n      path: /\n      target: user-api\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("bad host")));

        // 重定向：合法（301）与 status 非法、目标非法
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /old\n      redirect: /new\n      redirect_status: 301\n"
        );
        assert!(validate_static(&ws(&text), &conf_of(&ws(&text))).is_empty());

        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /old\n      redirect: http://ex.com/new\n      redirect_status: 303\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("redirect_status")));

        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /old\n      redirect: ftp://ex.com\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("redirect")));

        // 静态站点：合法；path != / 拒绝；越界拒绝
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      static_dir: web/dist\n"
        );
        assert!(validate_static(&ws(&text), &conf_of(&ws(&text))).is_empty());

        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /app\n      static_dir: dist\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("必须为 /")));

        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      static_dir: ../outside\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("..")));

        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /\n      static_dir: C:/abs\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("static_dir")));

        // cors 加在重定向/静态路由 → 拒绝
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /old\n      redirect: /new\n      cors:\n        origins:\n          - \"*\"\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues
            .iter()
            .any(|i| i.message.contains("cors 仅适用于代理路由")));

        // 混合形态（redirect + target）→ 拒绝
        let text = format!(
            "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /old\n      redirect: /new\n      target: user-api\n"
        );
        let issues = validate_static(&ws(&text), &conf_of(&ws(&text)));
        assert!(issues.iter().any(|i| i.message.contains("恰选其一")));
    }

    #[test]
    fn cors_validation_rules() {
        // 合法：通配、显式 origin、带凭据
        let mk = |cors: &str| {
            let text = format!(
                "{BASE}gateway:\n  kind: nginx\n  routes:\n    - path: /api\n      target: user-api\n{cors}"
            );
            validate_static(&ws(&text), &conf_of(&ws(&text)))
        };
        assert!(mk("      cors:\n        origins:\n          - \"*\"\n").is_empty());
        assert!(mk(
            "      cors:\n        origins:\n          - http://localhost:3000\n          - https://app.example.com:8443\n        methods:\n          - GET\n          - POST\n        max_age_secs: 3600\n        credentials: true\n"
        )
        .is_empty());

        // origins 为空
        let issues = mk("      cors: {}\n");
        assert!(issues
            .iter()
            .any(|i| i.message.contains("origins 不能为空")));

        // * 与其他 origin 混用
        let issues =
            mk("      cors:\n        origins:\n          - \"*\"\n          - http://a.com\n");
        assert!(issues.iter().any(|i| i.message.contains("混用")));

        // * + credentials 拒绝
        let issues =
            mk("      cors:\n        origins:\n          - \"*\"\n        credentials: true\n");
        assert!(issues.iter().any(|i| i.message.contains("凭据")));

        // origin 语法非法
        let issues = mk("      cors:\n        origins:\n          - http://a.com/path\n");
        assert!(issues.iter().any(|i| i.message.contains("origin")));

        // max_age 超上限
        let issues = mk(
            "      cors:\n        origins:\n          - \"*\"\n        max_age_secs: 999999999\n",
        );
        assert!(issues.iter().any(|i| i.message.contains("max_age_secs")));

        // methods 含空白
        let issues = mk(
            "      cors:\n        origins:\n          - \"*\"\n        methods:\n          - \"GET POST\"\n",
        );
        assert!(issues.iter().any(|i| i.message.contains("methods")));
    }
}
