//! 1.1 内置官方模板集成测试：只走公开 API（`list_templates` / `create_template`
//! + `parse_yaml`），模拟 Tauri 壳层的调用方式。

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use supertask_core::error::ErrorCode;
use supertask_core::spec::parse_yaml;
use supertask_core::template::{create_template, list_templates, TemplateSourceKind};

fn temp_parent(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-tpl-it-{}-{tag}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn list_offers_both_builtin_templates() {
    let list = list_templates(None);
    let ids: Vec<&str> = list.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&"spring-multimodule-node"), "ids: {ids:?}");
    assert!(
        ids.contains(&"spring-multimodule-node-minimal"),
        "ids: {ids:?}"
    );
    assert!(ids.contains(&"spring-boot-single"), "ids: {ids:?}");
    assert!(ids.contains(&"node-fullstack"), "ids: {ids:?}");

    for t in &list {
        assert_eq!(t.version, "1");
        assert_eq!(t.source, TemplateSourceKind::Builtin);
        assert!(!t.invalid);
        assert!(!t.stacks.is_empty());
        assert!(!t.name.is_empty());
        assert!(!t.description.is_empty());
        // 普通模板的文件概览必须包含核心文件；组合模板（blocks）不含
        // supertask.yaml——yaml 由块片段在创建时生成（模板升级后的设计）
        if t.blocks.is_none() {
            assert!(
                t.files.iter().any(|f| f == "supertask.yaml"),
                "{}: {:?}",
                t.id,
                t.files
            );
        }
        assert!(t
            .files
            .iter()
            .all(|f| !f.starts_with('/') && !f.contains('\\')));
    }
    // 各模板技术栈抽查
    let stacks_of = |id: &str| list.iter().find(|t| t.id == id).unwrap().stacks.clone();
    assert!(stacks_of("spring-boot-single") == vec!["spring-boot".to_string()]);
    assert!(stacks_of("node-fullstack") == vec!["node".to_string()]);
    assert!(stacks_of("spring-multimodule-node").contains(&"spring-boot".to_string()));
}

#[test]
fn create_full_template_and_parse_yaml() {
    let parent = temp_parent("full");
    let target = create_template(
        "spring-multimodule-node",
        TemplateSourceKind::Builtin,
        &parent,
        "ws-full",
        None,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(target.is_dir());

    let text = fs::read_to_string(target.join("supertask.yaml")).unwrap();
    let (file, warnings) = parse_yaml(&text).unwrap();
    assert!(
        warnings.is_empty(),
        "完整模板 YAML 不应产生告警: {warnings:?}"
    );

    // templates 保留段
    let tpl = file.templates.as_ref().expect("templates 段缺失");
    let m = tpl.as_mapping().unwrap();
    assert_eq!(
        m.get(serde_yaml::Value::from("source"))
            .and_then(|v| v.as_str()),
        Some("builtin")
    );

    // 双服务 + 依赖关系 + 健康检查
    assert_eq!(file.services.len(), 2);
    let backend = file.services.get("backend").unwrap();
    assert_eq!(backend.kind, "spring-boot");
    assert_eq!(backend.port, Some(8081));
    let health = backend.health.as_ref().unwrap();
    assert_eq!(
        health.http.as_deref(),
        Some("http://127.0.0.1:8081/actuator/health")
    );
    let web = file.services.get("web").unwrap();
    assert_eq!(web.depends_on, vec!["backend"]);
    assert_eq!(web.port, Some(5173));

    // 未知 id → NOT_FOUND；非空目标 → TARGET_NOT_EMPTY
    let err = create_template(
        "nope",
        TemplateSourceKind::Builtin,
        &parent,
        "another",
        None,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::NotFound);
    let err = create_template(
        "spring-multimodule-node",
        TemplateSourceKind::Builtin,
        &parent,
        "ws-full",
        None,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
    )
    .unwrap_err();
    assert_eq!(err.code(), ErrorCode::TargetNotEmpty);

    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn create_minimal_template_backend_health_falls_back() {
    let parent = temp_parent("min");
    let target = create_template(
        "spring-multimodule-node-minimal",
        TemplateSourceKind::Builtin,
        &parent,
        "ws-min",
        None,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
    )
    .unwrap();

    // 模板自带 yaml 精简：backend 不含 health 键（结构化检查，忽略注释）
    let raw = fs::read_to_string(target.join("supertask.yaml")).unwrap();
    let doc: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
    assert!(
        doc.get("services")
            .and_then(|s| s.get("backend"))
            .unwrap()
            .get("health")
            .is_none(),
        "最小模板 backend 不应自带 health 字段"
    );

    // parse_yaml 的 apply_defaults 为 backend 兜底 TCP 健康检查（spring-boot 缺省，
    // 与扫描的无 actuator 分支一致；HTTP/actuator 探测需 yaml 显式配置）
    let (file, _) = parse_yaml(&raw).unwrap();
    assert_eq!(file.services.len(), 2);
    let backend = file.services.get("backend").unwrap();
    let health = backend
        .health
        .as_ref()
        .expect("apply_defaults 应兜底 health");
    assert_eq!(health.r#type, supertask_core::spec::HealthType::Tcp);
    assert!(health.http.is_none());

    let _ = fs::remove_dir_all(&parent);
}

#[test]
fn create_spring_single_and_node_fullstack() {
    let parent = temp_parent("new-templates");

    // spring-boot-single：module "." + project_name 参数覆写 yaml.name
    let mut params = std::collections::BTreeMap::new();
    params.insert("project_name".to_string(), "my-single-app".to_string());
    let target = create_template(
        "spring-boot-single",
        TemplateSourceKind::Builtin,
        &parent,
        "ws-single",
        None,
        &params,
        None,
        &BTreeMap::new(),
    )
    .unwrap();
    assert!(target.join("pom.xml").is_file());
    let text = fs::read_to_string(target.join("supertask.yaml")).unwrap();
    let (file, _) = parse_yaml(&text).unwrap();
    assert_eq!(file.name.as_deref(), Some("my-single-app"));
    let app = file.services.get("app").unwrap();
    assert_eq!(app.kind, "spring-boot");
    assert_eq!(app.module.as_deref(), Some("."));
    assert_eq!(app.port, Some(8080));
    let health = app.health.as_ref().unwrap();
    assert_eq!(
        health.http.as_deref(),
        Some("http://127.0.0.1:8080/actuator/health")
    );

    // node-fullstack：api + web 双服务 + depends_on
    let target = create_template(
        "node-fullstack",
        TemplateSourceKind::Builtin,
        &parent,
        "ws-node",
        None,
        &BTreeMap::new(),
        None,
        &BTreeMap::new(),
    )
    .unwrap();
    let text = fs::read_to_string(target.join("supertask.yaml")).unwrap();
    let (file, _) = parse_yaml(&text).unwrap();
    assert_eq!(file.services.len(), 2);
    let web = file.services.get("web").unwrap();
    assert_eq!(web.depends_on, vec!["api"]);
    assert_eq!(file.services.get("api").unwrap().port, Some(3001));

    let _ = fs::remove_dir_all(&parent);
}
