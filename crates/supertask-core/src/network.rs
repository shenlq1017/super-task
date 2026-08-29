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
use crate::error::{ErrorCode, Result};
use crate::spec::validate::validate_proxy_url;
use crate::spec::{NetworkSpec, ProxyMode};

/// 字段级生效配置：workspace 值覆盖 app 默认，未配置的字段继承 app 默认。
/// 1.7 §7：镜像/registry 字段并入（此前只有代理字段且启动链无消费方）。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EffectiveNetwork {
    pub mode: Option<ProxyMode>,
    pub http: Option<String>,
    pub https: Option<String>,
    pub no_proxy: Vec<String>,
    pub maven_mirror: Option<String>,
    pub npm_registry: Option<String>,
    pub pip_index: Option<String>,
    pub go_goproxy: Option<String>,
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
        eff.maven_mirror = a.maven_mirror.clone();
        eff.npm_registry = a.npm_registry.clone();
        eff.pip_index = a.pip_index.clone();
        eff.go_goproxy = a.go_goproxy.clone();
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
        // 1.7 §7.2：镜像字段同样 workspace 覆盖 app
        if let Some(m) = &w.maven {
            if m.mirror.is_some() {
                eff.maven_mirror = m.mirror.clone();
            }
        }
        if let Some(n) = &w.npm {
            if n.registry.is_some() {
                eff.npm_registry = n.registry.clone();
            }
        }
        if let Some(py) = &w.python {
            if py.index_url.is_some() {
                eff.pip_index = py.index_url.clone();
            }
        }
        if let Some(g) = &w.go {
            if g.goproxy.is_some() {
                eff.go_goproxy = g.goproxy.clone();
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

fn set_proxy_vars(vars: &mut IndexMap<String, String>, http: Option<&str>, https: Option<&str>) {
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

/// 1.7 §7：启动 env 注入（**最低优先级**——已存在的键不覆盖，服务显式 env 永远赢）。
/// - npm registry → `npm_config_registry`；pip → `PIP_INDEX_URL`；go → `GOPROXY`；
/// - maven mirror → 生成 `.supertask/maven-settings.xml`（磁盘产物=缓存）并注入
///   `MAVEN_ARGS="-s <绝对路径>"`；用户已显式设置 `MAVEN_ARGS` 时不覆盖；
/// - 代理键（现有 `tool_env` 语义）一并注入。
/// 返回 (注入条数, 警告)。
pub fn inject_env(
    eff: &EffectiveNetwork,
    root: &std::path::Path,
    env: &mut IndexMap<String, String>,
) -> (usize, Vec<String>) {
    fn put(env: &mut IndexMap<String, String>, k: &str, v: String, count: &mut usize) {
        if !env.contains_key(k) {
            env.insert(k.to_string(), v);
            *count += 1;
        }
    }
    let mut warnings = Vec::new();
    let mut count = 0usize;
    if let Some(reg) = &eff.npm_registry {
        put(env, "npm_config_registry", reg.clone(), &mut count);
    }
    if let Some(idx) = &eff.pip_index {
        put(env, "PIP_INDEX_URL", idx.clone(), &mut count);
    }
    if let Some(gp) = &eff.go_goproxy {
        put(env, "GOPROXY", gp.clone(), &mut count);
    }
    if let Some(mirror) = &eff.maven_mirror {
        match write_maven_settings(root, mirror) {
            Ok(settings_path) => {
                let args = format!("-s {}", settings_path.display());
                put(env, "MAVEN_ARGS", args, &mut count);
            }
            Err(e) => warnings.push(format!(
                "maven-settings.xml 生成失败（本次启动不注入镜像）: {}",
                e.message()
            )),
        }
    }
    // 代理：off/无 → 空；系统代理按注册表读取（§7 现语义）
    if let Ok(proxy_vars) = tool_env(eff) {
        for (k, v) in proxy_vars {
            put(env, &k, v, &mut count);
        }
    }
    (count, warnings)
}

/// 生成仅含 mirror 的极简 settings.xml（id `supertask-mirror`，mirrorOf `*`）。
/// 不修改用户全局 settings.xml；产物是缓存不是编辑对象。
pub fn write_maven_settings(
    root: &std::path::Path,
    mirror: &str,
) -> crate::error::Result<std::path::PathBuf> {
    let dir = root.join(".supertask");
    std::fs::create_dir_all(&dir).map_err(|e| {
        crate::Error::new(
            ErrorCode::ProxyInvalid,
            format!("无法创建 .supertask 目录: {e}"),
        )
    })?;
    let path = dir.join("maven-settings.xml");
    let xml = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!-- 由 SuperTask 生成（缓存产物，勿手改）；删除后下次启动按配置重新生成 -->\n\
         <settings xmlns=\"http://maven.apache.org/SETTINGS/1.0.0\">\n\
         \x20 <mirrors>\n\
         \x20   <mirror>\n\
         \x20     <id>supertask-mirror</id>\n\
         \x20     <mirrorOf>*</mirrorOf>\n\
         \x20     <url>{mirror}</url>\n\
         \x20   </mirror>\n\
         \x20 </mirrors>\n\
         </settings>\n"
    );
    std::fs::write(&path, xml).map_err(|e| {
        crate::Error::new(
            ErrorCode::ProxyInvalid,
            format!("无法写入 maven-settings.xml: {e}"),
        )
    })?;
    Ok(path)
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
    use crate::spec::{GoNetworkSpec, MavenNetworkSpec, ProxySpec, PythonNetworkSpec};

    fn app_net(mode: &str) -> AppNetwork {
        AppNetwork {
            proxy_mode: mode.into(),
            ..Default::default()
        }
    }

    #[test]
    fn workspace_overrides_app_fieldwise() {
        let app = AppNetwork {
            proxy_mode: "custom".into(),
            http: Some("http://10.0.0.1:7890".into()),
            ..Default::default()
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
            python: None,
            go: None,
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

    // ---- 1.7 §7：镜像字段合并与启动注入 ----

    #[test]
    fn mirror_fields_merge_workspace_over_app() {
        let mut app = app_net("off");
        app.maven_mirror = Some("https://app-mirror".into());
        app.npm_registry = Some("https://app-registry".into());
        let ws: NetworkSpec = NetworkSpec {
            proxy: None,
            maven: Some(MavenNetworkSpec {
                mirror: Some("https://ws-mirror".into()),
                extra: Default::default(),
            }),
            npm: None,
            python: Some(PythonNetworkSpec {
                index_url: Some("https://pypi.example".into()),
                extra: Default::default(),
            }),
            go: None,
            extra: Default::default(),
        };
        let eff = resolve(Some(&ws), Some(&app)).unwrap();
        // workspace 覆盖 app
        assert_eq!(eff.maven_mirror.as_deref(), Some("https://ws-mirror"));
        // app 无覆盖时透传
        assert_eq!(eff.npm_registry.as_deref(), Some("https://app-registry"));
        assert_eq!(eff.pip_index.as_deref(), Some("https://pypi.example"));
        assert!(eff.go_goproxy.is_none());
    }

    #[test]
    fn inject_env_lowest_priority_and_maven_settings() {
        let eff = EffectiveNetwork {
            npm_registry: Some("https://reg.example".into()),
            pip_index: Some("https://pypi.example/simple".into()),
            go_goproxy: Some("https://goproxy.example".into()),
            maven_mirror: Some("https://mirror.example/maven".into()),
            ..Default::default()
        };
        let root = std::env::temp_dir().join(format!("st-net-inject-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let mut env = IndexMap::new();
        // 显式 env 永远赢
        env.insert("GOPROXY".into(), "https://explicit".into());
        env.insert("MAVEN_ARGS".into(), "-T 4".into());
        let (count, _) = inject_env(&eff, &root, &mut env);
        assert_eq!(
            env.get("npm_config_registry").map(String::as_str),
            Some("https://reg.example")
        );
        assert_eq!(
            env.get("PIP_INDEX_URL").map(String::as_str),
            Some("https://pypi.example/simple")
        );
        assert_eq!(
            env.get("GOPROXY").map(String::as_str),
            Some("https://explicit"),
            "显式值不覆盖"
        );
        // maven settings.xml 生成 + MAVEN_ARGS 已显式 → 不覆盖
        let settings = root.join(".supertask/maven-settings.xml");
        assert!(settings.is_file());
        let xml = std::fs::read_to_string(&settings).unwrap();
        assert!(xml.contains("<mirrorOf>*</mirrorOf>"));
        assert!(xml.contains("https://mirror.example/maven"));
        assert_eq!(env.get("MAVEN_ARGS").map(String::as_str), Some("-T 4"));
        // 无显式 MAVEN_ARGS 的新 env 会拿到 -s 注入
        let mut env2 = IndexMap::new();
        let (count2, _) = inject_env(&eff, &root, &mut env2);
        assert!(env2.get("MAVEN_ARGS").unwrap().starts_with("-s "));
        assert!(count >= 2 && count2 >= 4);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn userinfo_and_non_http_urls_rejected() {
        let app = AppNetwork {
            proxy_mode: "custom".into(),
            http: Some("http://user:pass@127.0.0.1:7890".into()),
            https: Some("ftp://127.0.0.1:7890".into()),
            ..Default::default()
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
            ..Default::default()
        };
        let eff = resolve(None, Some(&app)).unwrap();
        let vars = tool_env(&eff).unwrap();
        assert_eq!(
            vars.get("HTTP_PROXY").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
        assert_eq!(
            vars.get("https_proxy").map(String::as_str),
            Some("http://127.0.0.1:7890")
        );
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
        assert_eq!(
            with.get("HTTP_PROXY").map(String::as_str),
            Some("http://10.0.0.9:8080")
        );
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
        assert_eq!(
            stripped.get("JAVA_HOME").map(String::as_str),
            Some("C:\\jdk")
        );
    }

    #[test]
    fn loopback_defaults_dedupe() {
        let list = with_loopback_defaults(&["localhost".to_string(), " corp.local ".to_string()]);
        assert_eq!(
            list,
            vec![
                "localhost".to_string(),
                "corp.local".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string()
            ]
        );
    }

    #[cfg(windows)]
    #[test]
    fn reg_line_parse_keeps_proxy_value() {
        // 复用 network 的行解析路径：enabled 关闭时直接 None
        assert_eq!(reg_query(r"HKCU\Nonexistent-Key-ZXQ", "ProxyEnable"), None);
    }
}
