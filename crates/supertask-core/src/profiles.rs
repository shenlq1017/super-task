//! 1.2 Profile overlay 与分组（规格 §10）。
//!
//! 有效配置 = base spec 叠加 active profile，**不把合并结果写回 base 字段**。
//! profile 只允许覆盖：工作区 env、服务的 `enabled`/`env`/`port`。
//! 没有 `profiles` 时使用隐式 `default`，不改写 YAML。

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::spec::{ServiceSpec, SuperTaskFile};

pub const DEFAULT_PROFILE: &str = "default";

/// active profile id；未配置时为隐式 `default`。
pub fn active_id(spec: &SuperTaskFile) -> String {
    spec.profiles
        .as_ref()
        .and_then(|p| p.active.clone())
        .unwrap_or_else(|| DEFAULT_PROFILE.to_string())
}

pub fn profile_ids(spec: &SuperTaskFile) -> Vec<String> {
    spec.profiles
        .as_ref()
        .map(|p| p.items.keys().cloned().collect())
        .unwrap_or_default()
}

fn active_item<'a>(spec: &'a SuperTaskFile, active: &str) -> Option<&'a crate::spec::ProfileItem> {
    spec.profiles.as_ref()?.items.get(active)
}

/// 工作区层 env：base env + active profile env（后者覆盖前者）。
pub fn workspace_env(spec: &SuperTaskFile) -> IndexMap<String, String> {
    let mut env = spec.env.clone();
    if let Some(item) = active_item(spec, &active_id(spec)) {
        for (k, v) in &item.env {
            env.insert(k.clone(), v.clone());
        }
    }
    env
}

/// 服务级有效配置：base 服务叠加 active profile 的 services[id] 覆盖。
/// port 覆盖按 §10.2 使用与端口修复一致的 env 键/健康 URL 更新规则。
pub fn effective_service(spec: &SuperTaskFile, id: &str) -> Result<ServiceSpec> {
    let svc = spec
        .services
        .get(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    let mut eff = svc.clone();
    let Some(item) = active_item(spec, &active_id(spec)) else {
        return Ok(eff);
    };
    let Some(ov) = item.services.get(id) else {
        return Ok(eff);
    };
    for (k, v) in &ov.env {
        eff.env.insert(k.clone(), v.clone());
    }
    if let Some(enabled) = ov.enabled {
        eff.enabled = enabled;
    }
    if let Some(new_port) = ov.port {
        if eff.port.is_some() && eff.port != Some(new_port) {
            let old = eff.port.unwrap();
            // 与 ports::apply_port_assign 相同的跟随规则
            let key = crate::ports::port_env_key(&eff.kind).map(str::to_string);
            if let Some(key) = &key {
                if let Some(v) = eff.env.get_mut(key) {
                    if v == &old.to_string() {
                        *v = new_port.to_string();
                    }
                }
            }
            if let Some(h) = &mut eff.health {
                if let Some(url) = &mut h.http {
                    for base in ["http://127.0.0.1:", "http://localhost:"] {
                        let prefix = format!("{base}{old}");
                        if url.starts_with(&prefix) {
                            *url = format!("{base}{new_port}{}", &url[prefix.len()..]);
                            break;
                        }
                    }
                }
            }
        }
        eff.port = Some(new_port);
    }
    Ok(eff)
}

/// 整文件视图：env 换成工作区层（含 profile），指定服务换成有效配置。
/// 供 plan_service / 端口建议等复用现有纯函数逻辑。
pub fn overlay_spec(spec: &SuperTaskFile, id: &str) -> Result<SuperTaskFile> {
    let mut eff_file = spec.clone();
    eff_file.env = workspace_env(spec);
    let eff = effective_service(spec, id)?;
    eff_file.services.insert(id.to_string(), eff);
    Ok(eff_file)
}

/// §10.2 切换前置：profile 必须存在。
pub fn require_profile(spec: &SuperTaskFile, id: &str) -> Result<()> {
    let profiles = spec.profiles.as_ref().map(|p| p.items.clone()).unwrap_or_default();
    if profiles.contains_key(id) {
        return Ok(());
    }
    Err(Error::new(
        ErrorCode::ProfileNotFound,
        format!("profile {id:?} 不存在。可用: {DEFAULT_PROFILE}（隐式）及 {:?}", profile_ids(spec)),
    ))
}

/// profiles.list 负载：active + 每个 profile 的 enabled 服务计数。
pub fn list(spec: &SuperTaskFile) -> crate::ipc::ProfilesListOutput {
    let active = active_id(spec);
    let mut out = Vec::new();
    for (pid, item) in spec.profiles.as_ref().map(|p| p.items.clone()).unwrap_or_default() {
        let mut enabled_count = 0u32;
        for (sid, svc) in &spec.services {
            let enabled = item
                .services
                .get(sid)
                .and_then(|ov| ov.enabled)
                .unwrap_or(svc.enabled);
            if enabled {
                enabled_count += 1;
            }
        }
        out.push(crate::ipc::ProfileSummary { id: pid, enabled_count: Some(enabled_count) });
    }
    crate::ipc::ProfilesListOutput { active, profiles: out }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;

    const Y: &str = r#"
version: 1
env:
  WS: base
services:
  api:
    kind: spring-boot
    module: api
    port: 8080
    env:
      SVC: base
    health:
      type: http
      http: http://127.0.0.1:8080/actuator/health
  web:
    kind: node
    dir: web
    port: 5173
profiles:
  active: local
  items:
    local:
      env:
        WS: local
        PROFILE_ONLY: 1
      services:
        api:
          port: 9090
          env:
            SVC: profiled
        web:
          enabled: false
    test: {}
"#;

    #[test]
    fn overlay_applies_env_port_enabled() {
        let spec = parse_yaml(Y).unwrap().0;
        assert_eq!(active_id(&spec), "local");

        let ws = workspace_env(&spec);
        assert_eq!(ws.get("WS").map(String::as_str), Some("local"));
        assert_eq!(ws.get("PROFILE_ONLY").map(String::as_str), Some("1"));

        let api = effective_service(&spec, "api").unwrap();
        assert_eq!(api.port, Some(9090));
        // profile service env 覆盖服务 env（§6.3 顺序）
        assert_eq!(api.env.get("SVC").map(String::as_str), Some("profiled"));
        // 端口覆盖按端口修复规则跟随默认健康 URL
        assert_eq!(
            api.health.as_ref().unwrap().http.as_deref(),
            Some("http://127.0.0.1:9090/actuator/health")
        );

        let web = effective_service(&spec, "web").unwrap();
        assert!(!web.enabled, "profile enabled:false 生效");
    }

    #[test]
    fn base_spec_never_mutated_and_default_when_absent() {
        let spec = parse_yaml("version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8080\n").unwrap().0;
        assert_eq!(active_id(&spec), DEFAULT_PROFILE);
        assert_eq!(effective_service(&spec, "api").unwrap().port, Some(8080));

        // overlay_spec 不改 base
        let spec2 = parse_yaml(Y).unwrap().0;
        let _ = overlay_spec(&spec2, "api").unwrap();
        assert_eq!(spec2.services.get("api").unwrap().port, Some(8080));
        assert_eq!(spec2.env.get("WS").map(String::as_str), Some("base"));
    }

    #[test]
    fn list_counts_enabled_and_require_profile() {
        let spec = parse_yaml(Y).unwrap().0;
        let out = list(&spec);
        assert_eq!(out.active, "local");
        let local = out.profiles.iter().find(|p| p.id == "local").unwrap();
        assert_eq!(local.enabled_count, Some(1), "web 被 profile 关闭");
        assert_eq!(
            require_profile(&spec, "nope").unwrap_err().code(),
            ErrorCode::ProfileNotFound
        );
        require_profile(&spec, "test").unwrap();
    }
}
