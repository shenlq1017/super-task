use supertask_core::error::ErrorCode;
use supertask_core::spec::{parse_yaml, to_yaml};

const SAMPLE: &str = r#"
version: 1
name: mall
env:
  SPRING_PROFILES_ACTIVE: local
  DEBUG: 1
services:
  user-api:
    kind: spring-boot
    module: user-service
    port: 8081
    depends_on: []
  web:
    kind: node
    dir: web
    port: 5173
    depends_on: [user-api]
  db:
    kind: compose
    service: mysql
scripts:
  bootstrap:
    desc: 安装依赖
    cmds:
      - mvn -q -DskipTests install
gateway: {}
x-custom: { foo: 1 }
"#;

#[test]
fn reserved_and_extra_round_trip() {
    let (file, warnings) = parse_yaml(SAMPLE).unwrap();
    assert!(file.gateway.is_some());
    assert!(file.extra.contains_key("x-custom"));
    // 1.3 起 kind: compose 是合法 kind（可解析；启动支持在 phase 3 接入），
    // 不再落入 KIND_UNSUPPORTED 警告分支
    assert!(!warnings
        .iter()
        .any(|w| w.code == ErrorCode::KindUnsupported));
    let text = to_yaml(&file).unwrap();
    let (file2, _) = parse_yaml(&text).unwrap();
    assert!(file2.gateway.is_some());
    assert!(file2.extra.contains_key("x-custom"));
    assert_eq!(file2.services.get("db").unwrap().kind, "compose");
    assert_eq!(file2.services.get("db").unwrap().service.as_deref(), Some("mysql"));
}

#[test]
fn health_non_loopback_rejected() {
    let e = parse_yaml(
        r#"
version: 1
services:
  api:
    kind: spring-boot
    module: api
    port: 8080
    health:
      type: http
      http: http://example.com/health
"#,
    )
    .unwrap_err();
    assert_eq!(e.code(), ErrorCode::HealthHostForbidden);
}

#[test]
fn jar_launch_round_trips() {
    let (file, warnings) = parse_yaml(
        r#"
version: 1
services:
  api:
    kind: spring-boot
    module: api
    port: 8080
    launch: jar
    build_args: ["-DskipTests"]
"#,
    )
    .unwrap();
    assert!(warnings
        .iter()
        .all(|w| w.code != ErrorCode::LaunchUnsupported));
    let api = file.services.get("api").unwrap();
    assert_eq!(api.launch.as_deref(), Some("jar"));
    assert_eq!(api.build_args, vec!["-DskipTests".to_string()]);
    let text = to_yaml(&file).unwrap();
    let (file2, _) = parse_yaml(&text).unwrap();
    let api2 = file2.services.get("api").unwrap();
    assert_eq!(api2.launch.as_deref(), Some("jar"));
    assert_eq!(api2.build_args, vec!["-DskipTests".to_string()]);
}

#[test]
fn numeric_env_becomes_string() {
    let (file, _) = parse_yaml(
        r#"
version: 1
env:
  PORT: 5173
services:
  web:
    kind: node
    dir: web
    port: 5173
"#,
    )
    .unwrap();
    assert_eq!(file.env.get("PORT").unwrap(), "5173");
}
