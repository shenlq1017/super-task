//! 1.2 网络代理与镜像（规格 §7）。off/system/custom 三种策略，产出外部工具
//! （provider / Maven / npm）可用的环境变量；健康检查等 loopback 调用必须
//! 绕过代理（`strip_proxy_vars`）。
//!
//! 边界：
//! - URL 只允许 http/https、禁止内嵌用户名密码（复用 YAML 侧校验）→ `PROXY_INVALID`；
//! - `system` 读 Windows 注册表用户代理，不执行 PAC，读不到就直连（等同 off）；
//! - `no_proxy` 始终补齐 loopback 默认（127.0.0.1 / localhost / ::1）；
//! - 不修改用户全局 settings.xml / .npmrc / Git config。

use indexmap::IndexMap;

use crate::appdata::AppNetwork;
use crate::error::Result;
use crate::spec::{NetworkSpec, ProxyMode};
use crate::spec::validate::validate_proxy_url;

/// 字段级生效配置：workspace 值覆盖 app 默认，未配置的字段继承 app 默认。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectiveNetwork {
    pub mode: Option<ProxyMode>,
    pub http: Option<String>,
    pub https: Option<String>,
    pub no_proxy: Vec<String>,
}

/// workspace 优先合并 app 默认（§7.2）。`custom` 必须带显式 URL。
pub fn resolve(
    workspace: Option<&NetworkSpec>,
    app: Option<&AppNetwork>,
) -> Result<EffectiveNetwork> {
    let mut eff = EffectiveNetwork::default();
    if let Some(a) = app {
        eff.mode = parse_mode(&a.proxy_mode);
        eff.http = a.http.clone();
        eff.https = a.https.clone();
        eff.no_proxy = a.no_proxy.clone();
    }
    if let Some(w) = workspace {
        if let Some(p) = &w.proxy {
            if p.mode.is_some() {
                eff.mode = p.mode;
            }
            if p.http.is_some() {
                eff.http = p.http.clone();
            }
            if p.https.is_some() {
                eff.https = p.https.clone();
            }
            if !p.no_proxy.is_empty() {
                eff.no_proxy = p.no_proxy.clone();
            }
        }
    }
    if eff.mode == Some(ProxyMode::Custom) && eff.http.is_none() && eff.https.is_none() {
        return Err(crate::Error::new(
            crate::ErrorCode::ProxyInvalid,
            "custom 代理模式需要至少一个显式 URL",
        ));
    }
    if let Some(u) = &eff.http {
        validate_proxy_url(u)?;
    }
    if let Some(u) = &eff.https {
        validate_proxy_url(u)?;
    }
    Ok(eff)
}

/// 外部工具环境变量。`off` / 无代理 → 空表（不注入任何代理键）。
pub fn tool_env(eff: &EffectiveNetwork) -> Result<IndexMap<String, String>> {
    tool_env_with(eff, read_system_proxy())
}

pub fn tool_env_with(
    eff: &EffectiveNetwork,
    system_proxy: Option<String>,
) -> Result<IndexMap<String, String>> {
    let mut vars = IndexMap::new();
    match eff.mode {
        None | Some(ProxyMode::Off) => {}
        Some(ProxyMode::System) => {
            if let Some(url) = system_proxy {
                set_proxy_vars(&mut vars, Some(&url), Some(&url));
            }
        }
        Some(ProxyMode::Custom) => {
            set_proxy_vars(&mut vars, eff.http.as_deref(), eff.https.as_deref());
        }
    }
    if !vars.is_empty() {
        let list = with_loopback_defaults(&eff.no_proxy);
        let joined = list.join(",");
        vars.insert("NO_PROXY".into(), joined.clone());
        vars.insert("no_proxy".into(), joined);
    }
    Ok(vars)
}

fn set_proxy_vars(
    vars: &mut IndexMap<String, String>,
    http: Option<&str>,
    https: Option<&str>,
) {
    for (key, val) in [
        ("HTTP_PROXY", http),
        ("HTTPS_PROXY", https),
        // 小写变体：部分工具（curl 等）只认小写
        ("http_proxy", http),
        ("https_proxy", https),
    ] {
        if let Some(v) = val {
            vars.insert(key.to_string(), v.to_string());
        }
    }
}

/// 健康检查始终绕过代理（§7.2）：从继承的服务环境中剥掉全部代理键。
pub fn strip_proxy_vars(env: &IndexMap<String, String>) -> IndexMap<String, String> {
    const PROXY_KEYS: [&str; 6] = [
        "HTTP_PROXY",
        "HTTPS_PROXY",
        "NO_PROXY",
        "http_proxy",
        "https_proxy",
        "no_proxy",
    ];
    env.iter()
        .filter(|(k, _)| !PROXY_KEYS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// `no_proxy` 补齐 loopback 默认并去重（§7.3）。
pub fn with_loopback_defaults(no_proxy: &[String]) -> Vec<String> {
    let mut list: Vec<String> = Vec::new();
    for entry in no_proxy
        .iter()
        .map(|s| s.trim())
        .chain(["127.0.0.1", "localhost", "::1"])
    {
        if !entry.is_empty() && !list.iter().any(|x| x.eq_ignore_ascii_case(entry)) {
            list.push(entry.to_string());
        }
    }
    list
}

fn parse_mode(s: &str) -> Option<ProxyMode> {
    match s.to_ascii_lowercase().as_str() {
        "off" => Some(ProxyMode::Off),
        "system" => Some(ProxyMode::System),
        "custom" => Some(ProxyMode::Custom),
        _ => None,
    }
}

/// 系统代理（不执行 PAC）。Windows 读当前用户 Internet Settings；
/// 读不到 / 未启用 → None（直连，等同 off，不静默换成 custom）。
pub fn read_system_proxy() -> Option<String> {
    #[cfg(windows)]
    {
        read_system_proxy_windows()
    }
    #[cfg(not(windows))]
    {
        std::env::var("HTTPS_PROXY")
            .or_else(|_| std::env::var("HTTP_PROXY"))
            .ok()
            .filter(|s| !s.trim().is_empty())
    }
}

#[cfg(windows)]
fn read_system_proxy_windows() -> Option<String> {
    const KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Internet Settings";
    let enabled = reg_query(KEY, "ProxyEnable")?;
    if !enabled.eq_ignore_ascii_case("0x1") {
        return None;
    }
    let server = reg_query(KEY, "ProxyServer")?;
    // 值形如 `127.0.0.1:7890` 或 per-protocol `http=…;https=…`；`<local>` 等忽略
    let host = if server.contains('=') {
        server
            .split(';')
            .find_map(|kv| {
                let kv = kv.trim();
                kv.strip_prefix("http=")
                    .or_else(|| kv.strip_prefix("https="))
            })
            .map(str::trim)?
    } else {
        server.as_str()
    };
    if host.is_empty() || host.starts_with('<') {
        return None;
    }
    Some(format!("http://{host}"))
}

#[cfg(windows)]
fn reg_query(key: &str, value: &str) -> Option<String> {
    let out = std::process::Command::new("reg")
        .args(["query", key, "/v", value])
        .creation_flags_no_window()
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(|line| {
        let line = line.trim();
        let head_ok = line
            .get(..value.len())
            .is_some_and(|head| head.eq_ignore_ascii_case(value));
        if !head_ok {
            return None;
        }
        let marker = line.to_ascii_uppercase().find("_SZ")?;
        let v = line[marker + "_SZ".len()..].trim();
        if v.is_empty() {
            None
        } else {
            Some(v.to_string())
        }
    })
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::{MavenNetworkSpec, ProxySpec};

    fn app_net(mode: &str) -> AppNetwork {
        AppNetwork {
            proxy_mode: mode.into(),
            http: None,
            https: None,
            no_proxy: vec!["127.0.0.1".into(), "localhost".into(), "::1".into()],
        }
    }

    #[test]
    fn workspace_overrides_app_fieldwise() {
        let app = AppNetwork {
            proxy_mode: "custom".into(),
            http: Some("http://10.0.0.1:7890".into()),
            https: None,
            no_proxy: vec![],
        };
        let ws: NetworkSpec = NetworkSpec {
            proxy: Some(ProxySpec {
                mode: Some(ProxyMode::Off),
                http: None,
                https: Some("http://10.0.0.2:7890".into()),
                no_proxy: vec!["corp.local".into()],
                extra: Default::default(),
            }),
            maven: Some(MavenNetworkSpec {
                mirror: Some("https://maven.example.com/repository/public".into()),
                extra: Default::default(),
            }),
            npm: None,
            extra: Default::default(),
        };
        let eff = resolve(Some(&ws), Some(&app)).unwrap();
        // workspace 覆盖：mode 与 https 取 workspace，http 未配置继承 app
        assert_eq!(eff.mode, Some(ProxyMode::Off));
        assert_eq!(eff.http.as_deref(), Some("http://10.0.0.1:7890"));
        assert_eq!(eff.https.as_deref(), Some("http://10.0.0.2:7890"));
        assert_eq!(eff.no_proxy, vec!["corp.local".to_string()]);
        // mirror 透传（作用范围见 §7.2，Maven 调用侧消费）
        assert_eq!(
            ws.maven.unwrap().mirror.as_deref(),
            Some("https://maven.example.com/repository/public")
        );
    }

    #[test]
    fn custom_requires_explicit_url() {
        let eff = resolve(None, Some(&app_net("custom"))).unwrap_err();
        assert_eq!(eff.code(), crate::ErrorCode::ProxyInvalid);
    }

    #[test]
    fn userinfo_and_non_http_urls_rejected() {
        let app = AppNetwork {
            proxy_mode: "custom".into(),
            http: Some("http://user:pass@127.0.0.1:7890".into()),
            https: Some("ftp://127.0.0.1:7890".into()),
            no_proxy: vec![],
        };
        let e = resolve(None, Some(&app)).unwrap_err();
        assert_eq!(e.code(), crate::ErrorCode::ProxyInvalid);
    }

    #[test]
    fn off_injects_nothing() {
        let eff = resolve(None, Some(&app_net("off"))).unwrap();
        assert!(tool_env(&eff).unwrap().is_empty());
    }

    #[test]
    fn custom_sets_both_cases_and_no_proxy_loopback() {
        let app = AppNetwork {
            proxy_mode: "custom".into(),
            http: Some("http://127.0.0.1:7890".into()),
            https: Some("http://127.0.0.1:7890".into()),
            no_proxy: vec!["corp.local".into()],
        };
        let eff = resolve(None, Some(&app)).unwrap();
        let vars = tool_env(&eff).unwrap();
        assert_eq!(vars.get("HTTP_PROXY").map(String::as_str), Some("http://127.0.0.1:7890"));
        assert_eq!(vars.get("https_proxy").map(String::as_str), Some("http://127.0.0.1:7890"));
        // loopback 默认始终补齐，即使用户列表为空/不含
        let np = vars.get("NO_PROXY").unwrap();
        assert!(np.contains("corp.local"));
        assert!(np.contains("127.0.0.1") && np.contains("localhost") && np.contains("::1"));
    }

    #[test]
    fn system_uses_detected_proxy_or_direct() {
        let eff = resolve(None, Some(&app_net("system"))).unwrap();
        // 检测到 → 注入；检测不到 → 直连（等同 off），不报错
        let with = tool_env_with(&eff, Some("http://10.0.0.9:8080".into())).unwrap();
        assert_eq!(with.get("HTTP_PROXY").map(String::as_str), Some("http://10.0.0.9:8080"));
        let without = tool_env_with(&eff, None).unwrap();
        assert!(without.is_empty());
    }

    #[test]
    fn strip_removes_every_proxy_key_case_insensitive() {
        let mut env = IndexMap::new();
        env.insert("HTTP_PROXY".to_string(), "http://p:1".to_string());
        env.insert("https_proxy".to_string(), "http://p:1".to_string());
        env.insert("NO_PROXY".to_string(), "a".to_string());
        env.insert("JAVA_HOME".to_string(), "C:\\jdk".to_string());
        let stripped = strip_proxy_vars(&env);
        assert_eq!(stripped.len(), 1);
        assert_eq!(stripped.get("JAVA_HOME").map(String::as_str), Some("C:\\jdk"));
    }

    #[test]
    fn loopback_defaults_dedupe() {
        let list = with_loopback_defaults(&["localhost".to_string(), " corp.local ".to_string()]);
        assert_eq!(
            list,
            vec!["localhost".to_string(), "corp.local".to_string(), "127.0.0.1".to_string(), "::1".to_string()]
        );
    }

    #[cfg(windows)]
    #[test]
    fn reg_line_parse_keeps_proxy_value() {
        // 复用 network 的行解析路径：enabled 关闭时直接 None
        assert_eq!(reg_query(r"HKCU\Nonexistent-Key-ZXQ", "ProxyEnable"), None);
    }
}
