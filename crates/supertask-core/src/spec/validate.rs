use std::collections::HashSet;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::is_valid_id;
use crate::sandbox::is_loopback_url;

use super::file::{
    check_limits, HealthType, ParseWarning, SuperTaskFile, MAX_GROUP_CHARS, MAX_PROFILES,
};

pub fn validate(file: &SuperTaskFile) -> Result<Vec<ParseWarning>> {
    let mut warnings = Vec::new();

    if file.version != 1 {
        if file.version > 1 {
            warnings.push(ParseWarning {
                code: ErrorCode::SpecNewer,
                message: format!("version {} 新于本引擎，未知字段可能未执行", file.version),
            });
        } else {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("不支持 version {}", file.version),
            ));
        }
    }

    if file.root != "." {
        return Err(Error::new(ErrorCode::SpecInvalid, "root 只允许 \".\""));
    }

    if file.services.is_empty() {
        return Err(Error::new(ErrorCode::SpecInvalid, "至少需要一个 service"));
    }

    check_limits(file)?;

    let ids: HashSet<&str> = file.services.keys().map(|s| s.as_str()).collect();
    let mut ports: Vec<(String, u16)> = Vec::new();

    for (id, svc) in &file.services {
        match svc.kind.as_str() {
            "spring-boot" => {
                if svc.module.as_deref().unwrap_or("").is_empty() {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("{id}: spring-boot 需要 module"),
                    ));
                }
                if let Some(launch) = &svc.launch {
                    if launch != "run" && launch != "jar" {
                        return Err(Error::new(
                            ErrorCode::LaunchUnsupported,
                            format!("{id}: launch '{launch}' 不支持"),
                        ));
                    }
                }
            }
            "node" => {
                if svc.dir.as_deref().unwrap_or("").is_empty() {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("{id}: node 需要 dir"),
                    ));
                }
            }
            "compose" => {
                validate_compose_service(id, svc)?;
            }
            _ => {
                warnings.push(ParseWarning {
                    code: ErrorCode::KindUnsupported,
                    message: format!("{id}: kind '{}' 本版不能启动，配置会保留", svc.kind),
                });
            }
        }

        for dep in &svc.depends_on {
            if !ids.contains(dep.as_str()) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id}: depends_on '{dep}' 不存在"),
                ));
            }
        }

        if let Some(port) = svc.port {
            if port == 0 {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id}: 非法 port"),
                ));
            }
            if let Some((other, _)) = ports.iter().find(|(_, p)| *p == port) {
                warnings.push(ParseWarning {
                    code: ErrorCode::PortDup,
                    message: format!("端口 {port} 重复：{other} 与 {id}"),
                });
            }
            ports.push((id.clone(), port));
        }

        if let Some(h) = &svc.health {
            match h.r#type {
                HealthType::Tcp | HealthType::Http if svc.port.is_none() => {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("{id}: tcp/http 健康检查需要 port"),
                    ));
                }
                HealthType::Http => {
                    if let Some(url) = &h.http {
                        if !is_loopback_url(url) {
                            return Err(Error::new(
                                ErrorCode::HealthHostForbidden,
                                format!("{id}: 健康检查只允许 127.0.0.1/localhost"),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(dir) = &svc.dir {
            crate::sandbox::assert_rel_safe(dir)?;
        }
        if let Some(cwd) = &svc.cwd {
            crate::sandbox::assert_rel_safe(cwd)?;
        }
        for ef in &svc.env_file {
            crate::sandbox::assert_rel_safe(ef)?;
        }
        if let Some(group) = &svc.group {
            if group.chars().count() > MAX_GROUP_CHARS {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id}: group 最长 {MAX_GROUP_CHARS} 个字符"),
                ));
            }
        }
    }

    for (id, script) in &file.scripts {
        if script.cmds.iter().any(|c| c.trim().is_empty()) {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("脚本 {id} 含空命令"),
            ));
        }
        if let Some(cwd) = &script.cwd {
            crate::sandbox::assert_rel_safe(cwd)?;
        }
    }

    validate_v12(file)?;
    validate_v13(file)?;

    Ok(warnings)
}

fn validate_v12(file: &SuperTaskFile) -> Result<()> {
    if let Some(tc) = &file.toolchain {
        for (label, ver) in [
            ("java", tc.java.as_deref()),
            ("maven", tc.maven.as_deref()),
            ("node", tc.node.as_deref()),
        ] {
            if let Some(v) = ver {
                if !is_valid_toolchain_version(v) {
                    return Err(Error::new(
                        ErrorCode::ToolchainVersionInvalid,
                        format!("toolchain.{label} 版本非法: {v}"),
                    ));
                }
            }
        }
    }

    if let Some(sec) = &file.secrets {
        if let Some(path) = &sec.file {
            crate::sandbox::assert_rel_safe(path)?;
        }
        for key in &sec.required {
            if !is_valid_secret_key(key) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("secrets.required 非法 key 名: {key}"),
                ));
            }
        }
    }

    if let Some(net) = &file.network {
        if let Some(proxy) = &net.proxy {
            if let Some(url) = &proxy.http {
                validate_proxy_url(url)?;
            }
            if let Some(url) = &proxy.https {
                validate_proxy_url(url)?;
            }
        }
        if let Some(maven) = &net.maven {
            if let Some(url) = &maven.mirror {
                validate_proxy_url(url)?;
            }
        }
        if let Some(reg) = &net.npm {
            if let Some(url) = &reg.registry {
                validate_proxy_url(url)?;
            }
        }
    }

    if let Some(profiles) = &file.profiles {
        if profiles.items.len() > MAX_PROFILES {
            return Err(Error::new(
                ErrorCode::ProfileInvalid,
                format!("profile 数量超过 {MAX_PROFILES}"),
            ));
        }
        for id in profiles.items.keys() {
            if !is_valid_id(id) {
                return Err(Error::new(
                    ErrorCode::ProfileInvalid,
                    format!("非法 profile id {id}"),
                ));
            }
        }
        if let Some(active) = &profiles.active {
            if !profiles.items.contains_key(active) {
                return Err(Error::new(
                    ErrorCode::ProfileNotFound,
                    format!("active profile '{active}' 不存在"),
                ));
            }
        }
        for (pid, item) in &profiles.items {
            for (sid, ov) in &item.services {
                if !is_valid_id(sid) {
                    return Err(Error::new(
                        ErrorCode::ProfileInvalid,
                        format!("profile {pid} 非法服务 id {sid}"),
                    ));
                }
                if let Some(0) = ov.port {
                    return Err(Error::new(
                        ErrorCode::ProfileInvalid,
                        format!("profile {pid} 服务 {sid} 非法 port"),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// 1.3 `docker` 段校验（§5.1/§6.2/§10.2 的静态部分）。
/// compose 文件与 service 名的存在性检查需要 `docker compose config`，
/// 属于 CLI 适配层（phase 2/3），这里只做字符集与沙箱规则。
fn validate_v13(file: &SuperTaskFile) -> Result<()> {
    use super::file::DockerSpec;

    if let Some(docker) = &file.docker {
        let DockerSpec {
            compose_file,
            project_name,
            builds,
            ..
        } = docker;
        if let Some(f) = compose_file {
            validate_rel_path(f, "docker.compose_file")?;
        }
        if let Some(p) = project_name {
            if !is_valid_compose_name(p) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("docker.project_name 非法: {p:?}（只允许字母数字开头，含 . _ -，≤64 字符）"),
                ));
            }
        }
        let mut names = std::collections::HashSet::new();
        for b in builds {
            if b.name.is_empty() || !names.insert(b.name.as_str()) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("docker.builds.name 为空或重复: {:?}", b.name),
                ));
            }
            validate_rel_path(&b.context, "docker.builds.context")?;
            if let Some(df) = &b.dockerfile {
                validate_rel_path(df, "docker.builds.dockerfile")?;
            }
            if b.tags.is_empty() {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("docker.builds.{} tags 至少一条", b.name),
                ));
            }
            for t in &b.tags {
                if t.starts_with("--")
                    || t.is_empty()
                    || !t
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | ':' | '/' | '-' | '_'))
                {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("docker.builds.{} tag 非法: {t:?}", b.name),
                    ));
                }
            }
        }
    }
    Ok(())
}

/// `kind: compose` 服务静态校验（§5.1）。存在性检查（compose 文件里有没有
/// 这个 service）在 phase 3 启动前置检查做，这里只管 YAML 自身。
fn validate_compose_service(
    id: &str,
    svc: &super::file::ServiceSpec,
) -> Result<()> {
    let Some(service) = svc.service.as_deref() else {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            format!("{id}: kind: compose 需要 service 字段"),
        ));
    };
    if !is_valid_compose_name(service) {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            format!("{id}: compose service 名非法: {service:?}"),
        ));
    }
    // 防止用户以为 SuperTask 会向容器注入环境：这些字段对 compose 服务非法
    let violations: Vec<&str> = [
        (!svc.env.is_empty()).then_some("env"),
        (!svc.env_file.is_empty()).then_some("env_file"),
        (!svc.extra_args.is_empty()).then_some("extra_args"),
        (!svc.build_args.is_empty()).then_some("build_args"),
        (!svc.jvm_args.is_empty()).then_some("jvm_args"),
        svc.cwd.is_some().then_some("cwd"),
        svc.restart.is_some().then_some("restart"),
        svc.module.is_some().then_some("module"),
        svc.dir.is_some().then_some("dir"),
        svc.package_manager.is_some().then_some("package_manager"),
        svc.launch.is_some().then_some("launch"),
    ]
    .iter()
    .filter_map(|x| *x)
    .collect();
    if !violations.is_empty() {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            format!(
                "{id}: kind: compose 不允许字段 {}（环境与重启策略由 compose 文件自管）",
                violations.join(", ")
            ),
        ));
    }
    Ok(())
}

/// compose 服务名 / project name：`^[A-Za-z0-9][A-Za-z0-9_.-]{0,63}$`（§5.1）。
pub fn is_valid_compose_name(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 64 || !b[0].is_ascii_alphanumeric() {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'-'))
}

/// 相对路径规则：禁空、绝对路径（/、\\、盘符）与 `..` 段（§10.2 沙箱前置）。
fn validate_rel_path(p: &str, label: &str) -> Result<()> {
    let bad = |why: &str| {
        Err(Error::new(
            ErrorCode::SpecInvalid,
            format!("{label} 必须是工作区内的相对路径（{why}）: {p:?}"),
        ))
    };
    if p.is_empty() {
        return bad("为空");
    }
    if p.starts_with('/') || p.starts_with('\\') {
        return bad("绝对路径");
    }
    let bytes = p.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return bad("盘符");
    }
    if p.split(['/', '\\']).any(|seg| seg == "..") {
        return bad("不允许 .. 段");
    }
    Ok(())
}

pub fn is_valid_toolchain_version(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    if s.eq_ignore_ascii_case("lts") {
        return true;
    }
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | '+' | '@'))
}

pub fn is_valid_secret_key(s: &str) -> bool {
    let b = s.as_bytes();
    if b.is_empty() || b.len() > 128 {
        return false;
    }
    let first = b[0];
    if !(first.is_ascii_alphabetic() || first == b'_') {
        return false;
    }
    b[1..]
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || *c == b'_')
}

pub fn validate_proxy_url(url: &str) -> Result<()> {
    let trimmed = url.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_whitespace) {
        return Err(Error::new(ErrorCode::ProxyInvalid, "代理 URL 为空或含空白"));
    }
    let lower = trimmed.to_ascii_lowercase();
    let rest = if let Some(r) = lower.strip_prefix("https://") {
        r
    } else if let Some(r) = lower.strip_prefix("http://") {
        r
    } else {
        return Err(Error::new(ErrorCode::ProxyInvalid, "URL 只允许 http/https"));
    };
    // original authority is same length prefix as rest
    let orig_rest = &trimmed[trimmed.len() - rest.len()..];
    let authority = orig_rest.split('/').next().unwrap_or(orig_rest);
    let authority = authority.split('?').next().unwrap_or(authority);
    if authority.contains('@') {
        return Err(Error::new(
            ErrorCode::ProxyInvalid,
            "URL 禁止内嵌用户名密码",
        ));
    }
    if authority.is_empty() {
        return Err(Error::new(ErrorCode::ProxyInvalid, "URL 缺少主机"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;

    fn svc_yaml(extra: &str) -> String {
        format!(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n{extra}\n"
        )
    }

    #[test]
    fn toolchain_extra_round_trips() {
        let y = svc_yaml("toolchain:\n  manager: auto\n  java: \"21\"\n  x-provider: temurin\n");
        let (f, _) = parse_yaml(&y).unwrap();
        let tc = f.toolchain.as_ref().unwrap();
        assert_eq!(tc.java.as_deref(), Some("21"));
        assert!(tc.extra.contains_key("x-provider"));
        let text = crate::spec::to_yaml(&f).unwrap();
        let (f2, _) = parse_yaml(&text).unwrap();
        assert!(f2.toolchain.unwrap().extra.contains_key("x-provider"));
    }

    #[test]
    fn secrets_required_names_only() {
        let y = svc_yaml("secrets:\n  backend: file\n  file: .env.local\n  required:\n    - DB_PASSWORD\n    - JWT_SECRET\n");
        let (f, _) = parse_yaml(&y).unwrap();
        let s = f.secrets.unwrap();
        assert_eq!(
            s.required,
            vec!["DB_PASSWORD".to_string(), "JWT_SECRET".to_string()]
        );
        let dumped = crate::spec::to_yaml(&crate::spec::parse_yaml(&y).unwrap().0).unwrap();
        assert!(dumped.contains("DB_PASSWORD"));
        assert!(!dumped.to_lowercase().contains("hunter"));
    }

    #[test]
    fn proxy_rejects_userinfo_and_non_http() {
        let y = svc_yaml(
            "network:\n  proxy:\n    mode: custom\n    http: http://user:pass@127.0.0.1:7890\n",
        );
        let e = parse_yaml(&y).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ProxyInvalid);
        let y2 =
            svc_yaml("network:\n  proxy:\n    mode: custom\n    http: socks5://127.0.0.1:1080\n");
        let e2 = parse_yaml(&y2).unwrap_err();
        assert_eq!(e2.code(), ErrorCode::ProxyInvalid);
        assert!(validate_proxy_url("http://127.0.0.1:7890").is_ok());
    }

    #[test]
    fn profiles_network_retention_group_round_trip() {
        let extra = concat!(
            "network:\n  proxy:\n    mode: off\n  x-corp: true\n",
            "profiles:\n  active: local\n  items:\n    local: {}\n  x-keep: 1\n",
            "log_retention:\n  max_files: 5\n  x-policy: local\n",
        );
        let y = svc_yaml(extra).replace("    port: 8080\n", "    port: 8080\n    group: backend\n    env_file:\n      - .env.local\n    launch: jar\n    build_args:\n      - -DskipTests\n");
        let (f, _) = parse_yaml(&y).unwrap();
        assert_eq!(
            f.services.get("api").unwrap().group.as_deref(),
            Some("backend")
        );
        assert_eq!(
            f.services.get("api").unwrap().launch.as_deref(),
            Some("jar")
        );
        assert_eq!(
            f.services.get("api").unwrap().build_args,
            vec!["-DskipTests".to_string()]
        );
        assert!(f.network.as_ref().unwrap().extra.contains_key("x-corp"));
        assert!(f.profiles.as_ref().unwrap().extra.contains_key("x-keep"));
        assert!(f
            .log_retention
            .as_ref()
            .unwrap()
            .extra
            .contains_key("x-policy"));
        let text = crate::spec::to_yaml(&f).unwrap();
        let (f2, _) = parse_yaml(&text).unwrap();
        assert!(f2.network.unwrap().extra.contains_key("x-corp"));
        assert!(f2.profiles.unwrap().extra.contains_key("x-keep"));
        assert!(f2.log_retention.unwrap().extra.contains_key("x-policy"));
        assert_eq!(
            f2.services.get("api").unwrap().launch.as_deref(),
            Some("jar")
        );
    }

    #[test]
    fn invalid_toolchain_version() {
        let y = svc_yaml("toolchain:\n  java: \"foo bar\"\n");
        let e = parse_yaml(&y).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ToolchainVersionInvalid);
    }

    // ---- 1.3：kind: compose 与 docker 段 ----

    fn compose_yaml(extra: &str) -> String {
        format!(
            "version: 1\nservices:\n  redis:\n    kind: compose\n    service: redis\n    port: 6379\n{extra}\n"
        )
    }

    #[test]
    fn compose_service_and_docker_section_round_trip() {
        let y = compose_yaml(concat!(
            "docker:\n",
            "  compose_file: compose.yaml\n",
            "  project_name: mall\n",
            "  builds:\n",
            "    - name: mall-user\n",
            "      context: user-service\n",
            "      tags:\n",
            "        - mall-user:local\n",
            "  x-extra: keep-me\n",
        ));
        let (f, warnings) = parse_yaml(&y).unwrap();
        // compose 是合法 kind，不再产生 KIND_UNSUPPORTED 警告
        assert!(!warnings.iter().any(|w| w.code == ErrorCode::KindUnsupported));
        assert_eq!(
            f.services.get("redis").unwrap().service.as_deref(),
            Some("redis")
        );
        let docker = f.docker.as_ref().unwrap();
        assert_eq!(docker.compose_file.as_deref(), Some("compose.yaml"));
        assert_eq!(docker.builds[0].name, "mall-user");
        assert_eq!(docker.builds[0].tags, vec!["mall-user:local".to_string()]);
        assert!(docker.extra.contains_key("x-extra"));
        let text = crate::spec::to_yaml(&f).unwrap();
        let (f2, _) = parse_yaml(&text).unwrap();
        let d2 = f2.docker.as_ref().unwrap();
        assert_eq!(d2.builds[0].tags, vec!["mall-user:local".to_string()]);
        assert!(d2.extra.contains_key("x-extra"));
        assert_eq!(f2.services.get("redis").unwrap().service.as_deref(), Some("redis"));
    }

    #[test]
    fn compose_requires_service_and_valid_charset() {
        let missing = "version: 1\nservices:\n  redis:\n    kind: compose\n    port: 6379\n";
        assert_eq!(parse_yaml(missing).unwrap_err().code(), ErrorCode::SpecInvalid);
        let bad = compose_yaml("").replace("service: redis", "service: \"re dis\"");
        assert_eq!(parse_yaml(&bad).unwrap_err().code(), ErrorCode::SpecInvalid);
        let leading_dash = compose_yaml("").replace("service: redis", "service: -redis");
        assert_eq!(parse_yaml(&leading_dash).unwrap_err().code(), ErrorCode::SpecInvalid);
    }

    #[test]
    fn compose_rejects_env_injection_fields() {
        for (field, yaml) in [
            ("env", compose_yaml("").replace("    port: 6379\n", "    port: 6379\n    env:\n      FOO: bar\n")),
            ("env_file", compose_yaml("").replace("    port: 6379\n", "    port: 6379\n    env_file:\n      - .env\n")),
            ("restart", compose_yaml("").replace("    port: 6379\n", "    port: 6379\n    restart: unless-stopped\n")),
            ("launch", compose_yaml("").replace("    port: 6379\n", "    port: 6379\n    launch: run\n")),
            ("jvm_args", compose_yaml("").replace("    port: 6379\n", "    port: 6379\n    jvm_args:\n      - -Xmx1g\n")),
        ] {
            let e = parse_yaml(&yaml).unwrap_err();
            assert_eq!(e.code(), ErrorCode::SpecInvalid, "{field}");
            assert!(e.to_string().contains(field), "{field}: {}", e);
        }
    }

    #[test]
    fn docker_section_path_and_tag_rules() {
        let escape = svc_yaml("docker:\n  compose_file: ../outside/compose.yaml\n");
        assert_eq!(parse_yaml(&escape).unwrap_err().code(), ErrorCode::SpecInvalid);
        let absolute = svc_yaml("docker:\n  compose_file: C:/work/compose.yaml\n");
        assert_eq!(parse_yaml(&absolute).unwrap_err().code(), ErrorCode::SpecInvalid);
        let bad_tag = svc_yaml(
            "docker:\n  builds:\n    - name: a\n      context: .\n      tags:\n        - \"--rm\"\n",
        );
        assert_eq!(parse_yaml(&bad_tag).unwrap_err().code(), ErrorCode::SpecInvalid);
        let no_tags = svc_yaml("docker:\n  builds:\n    - name: a\n      context: .\n      tags: []\n");
        assert_eq!(parse_yaml(&no_tags).unwrap_err().code(), ErrorCode::SpecInvalid);
        let dup_names = svc_yaml(
            "docker:\n  builds:\n    - name: a\n      context: .\n      tags: [a:local]\n    - name: a\n      context: b\n      tags: [b:local]\n",
        );
        assert_eq!(parse_yaml(&dup_names).unwrap_err().code(), ErrorCode::SpecInvalid);
        let bad_project = svc_yaml("docker:\n  project_name: \"-mall\"\n");
        assert_eq!(parse_yaml(&bad_project).unwrap_err().code(), ErrorCode::SpecInvalid);
    }

    #[test]
    fn compose_name_validator_charset() {
        assert!(is_valid_compose_name("redis"));
        assert!(is_valid_compose_name("mall.db_1"));
        assert!(!is_valid_compose_name(""));
        assert!(!is_valid_compose_name("-lead"));
        assert!(!is_valid_compose_name("a b"));
        assert!(!is_valid_compose_name(&"x".repeat(65)));
    }
}
