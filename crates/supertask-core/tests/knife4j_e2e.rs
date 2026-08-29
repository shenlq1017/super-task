//! E2E against knife4j-demo-openapi3. Run: cargo test -p supertask-core --test knife4j_e2e -- --ignored --nocapture
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supertask_core::health;
use supertask_core::probe::probe_toolchain;
use supertask_core::runtime::RtState;
use supertask_core::scan::scan_draft;
use supertask_core::spec::{HealthSpec, HealthType};
use supertask_core::Engine;

const DEMO: &str = r"<knife4j-root>\knife4j\knife4j-demo-openapi3";
const PARENT: &str = r"<knife4j-root>\knife4j";

fn demo_exists() -> bool {
    Path::new(DEMO).join("pom.xml").is_file()
}

#[test]
fn scan_demo_dir_is_single_module() {
    if !demo_exists() {
        eprintln!("skip: knife4j demo not found at {DEMO}");
        return;
    }
    let (spec, _warnings) = scan_draft(Path::new(DEMO)).expect("demo scan");
    assert!(spec.services.contains_key("knife4j-demo-openapi3"));
    let svc = spec.services.get("knife4j-demo-openapi3").unwrap();
    // 单模块（带本地 parent）：module="." 省略 -pl，无需 reactor 根即可 spring-boot:run
    assert_eq!(svc.module.as_deref(), Some("."));
    assert_eq!(svc.cwd, None);
    assert_eq!(svc.health.as_ref().unwrap().r#type, HealthType::Tcp);
}

#[test]
fn scan_parent_finds_both_demos() {
    if !demo_exists() {
        return;
    }
    let (spec, _) = scan_draft(Path::new(PARENT)).expect("parent scan");
    assert!(
        spec.services.contains_key("knife4j-demo-openapi3"),
        "keys: {:?}",
        spec.services.keys().collect::<Vec<_>>()
    );
    assert!(spec.services.contains_key("knife4j-demo-openapi2"));
    let demo = spec.services.get("knife4j-demo-openapi3").unwrap();
    assert_eq!(demo.module.as_deref(), Some("knife4j-demo-openapi3"));
    assert_eq!(demo.health.as_ref().unwrap().r#type, HealthType::Tcp);
}

#[test]
#[ignore = "manual e2e: needs knife4j repo + JDK/Maven; may take 2+ min"]
fn engine_start_demo_and_call_api() {
    if !demo_exists() {
        eprintln!("skip: demo not at {DEMO}");
        return;
    }

    let probe = probe_toolchain();
    assert!(probe.java.found, "java missing: {:?}", probe.java);
    assert!(probe.maven.found, "maven missing: {:?}", probe.maven);
    eprintln!(
        "toolchain: java={:?} maven={:?}",
        probe.java.version, probe.maven.version
    );

    let yaml_path = PathBuf::from(PARENT).join("supertask.yaml");
    let yaml_backup = yaml_path
        .exists()
        .then(|| fs::read_to_string(&yaml_path).unwrap());
    let yaml = r#"version: 1
name: knife4j-demo-openapi3
services:
  knife4j-demo-openapi3:
    kind: spring-boot
    module: knife4j-demo-openapi3
    port: 8080
    grace_secs: 120
    health:
      type: http
      http: http://127.0.0.1:8080/v3/api-docs
      interval_secs: 3
      timeout_secs: 5
"#;
    struct YamlCleanup {
        path: PathBuf,
        backup: Option<String>,
    }
    impl Drop for YamlCleanup {
        fn drop(&mut self) {
            if let Some(b) = self.backup.take() {
                let _ = fs::write(&self.path, b);
            } else {
                let _ = fs::remove_file(&self.path);
            }
        }
    }

    let _guard = YamlCleanup {
        path: yaml_path.clone(),
        backup: yaml_backup,
    };
    fs::write(&yaml_path, yaml).expect("write supertask.yaml");

    let eng = Engine::new();
    let (warnings, snap) = eng.open(Path::new(PARENT)).expect("workspace.open");
    eprintln!("open warnings: {:?}", warnings);
    eprintln!(
        "initial snapshot: {:?}",
        snap.services.get("knife4j-demo-openapi3")
    );

    eng.subscribe_logs().expect("logs.subscribe");
    eng.start_one("knife4j-demo-openapi3")
        .expect("runtime.startOne");

    let id = "knife4j-demo-openapi3";
    let deadline = Instant::now() + Duration::from_secs(180);
    let mut last_state = RtState::Stopped;
    while Instant::now() < deadline {
        if let Some(supertask_core::EngineEvent::Runtime(rt)) = eng.try_recv_event() {
            if let Some(s) = rt.services.get(id) {
                if s.state != last_state {
                    eprintln!(
                        "state -> {:?} pid={:?} err={:?}",
                        s.state, s.pid, s.last_error
                    );
                    last_state = s.state;
                }
            }
        }
        if let Some(s) = eng
            .snapshot()
            .ok()
            .and_then(|s| s.services.get(id).cloned())
        {
            if s.state == RtState::Running {
                break;
            }
            if s.state == RtState::Exited {
                let logs = eng
                    .logs_snapshot(None, 40)
                    .map(|(l, _)| l)
                    .unwrap_or_default();
                for line in logs.iter().take(20) {
                    eprintln!("[{:?}] {}", line.stream, line.text);
                }
                panic!("service exited: {:?}", s.last_error);
            }
        }
        std::thread::sleep(Duration::from_millis(500));
    }

    let snap = eng.snapshot().expect("runtime.snapshot");
    let svc = snap.services.get(id).expect("service in snapshot");
    assert_eq!(
        svc.state,
        RtState::Running,
        "last_error={:?}",
        svc.last_error
    );

    let health = HealthSpec {
        r#type: HealthType::Http,
        http: Some("http://127.0.0.1:8080/v3/api-docs".into()),
        interval_secs: 2,
        timeout_secs: 5,
    };
    let h = health::check(&health, Some(8080));
    assert!(h.ok, "health: {}", h.detail);

    let user = health::check(
        &HealthSpec {
            r#type: HealthType::Http,
            http: Some("http://127.0.0.1:8080/api/user/list".into()),
            interval_secs: 2,
            timeout_secs: 5,
        },
        Some(8080),
    );
    assert!(user.ok, "user list: {}", user.detail);
    eprintln!("API ok: /v3/api-docs + /api/user/list");

    eng.stop_one(id).expect("runtime.stopOne");
    let stop_deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < stop_deadline {
        let stopped = eng
            .snapshot()
            .ok()
            .and_then(|s| s.services.get(id).map(|v| v.state == RtState::Stopped))
            .unwrap_or(false);
        if stopped {
            break;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
    eng.close().expect("workspace.close");
}
