//! 1.4 §5 Gradle 多模块：bootRun argv（wrapper 优先）、wrapper/PATH 双缺失、
//! bootJar 构建失败。用 fake gradlew 桩，不依赖真实 Gradle（外部 GUI 隔离守则：
//! 只 spawn 桩脚本，不弹任何系统窗口）。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use supertask_core::error::ErrorCode;
use supertask_core::probe::find_on_path;
use supertask_core::runtime::RtState;
use supertask_core::Engine;

fn ws_dir(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-gradle-{}-{}", tag, std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn java_available() -> bool {
    find_on_path("java.exe").is_some() || find_on_path("java").is_some()
}

/// 写 wrapper 桩：把任务参数落到工作区根 `.gradle-fake-args.txt` 后退出 0。
fn write_capture_stub(root: &Path) {
    #[cfg(windows)]
    fs::write(
        root.join("gradlew.bat"),
        "@echo off\r\n(echo %*)> .gradle-fake-args.txt\r\n",
    )
    .unwrap();
    #[cfg(not(windows))]
    {
        use std::os::unix::fs::PermissionsExt;
        let p = root.join("gradlew");
        fs::write(
            &p,
            "#!/bin/sh\nprintf '%s\\n' \"$@\" > .gradle-fake-args.txt\n",
        )
        .unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

fn wait_state(eng: &Engine, id: &str, want: RtState, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if let Ok(snap) = eng.snapshot() {
            if snap.services.get(id).map(|s| s.state) == Some(want) {
                return true;
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    false
}

/// bootRun：wrapper 优先（root gradlew.bat），argv 为 `:module:bootRun`。
#[test]
fn bootrun_uses_wrapper_with_task_path() {
    if !java_available() {
        eprintln!("skip: java 不在 PATH，无法走 Real spawner 前置检查");
        return;
    }
    let root = ws_dir("bootrun");
    fs::write(root.join("settings.gradle"), "include 'user-service'\n").unwrap();
    fs::create_dir_all(root.join("user-service")).unwrap();
    fs::write(
        root.join("user-service/build.gradle"),
        "plugins { id 'org.springframework.boot' }\n",
    )
    .unwrap();
    write_capture_stub(&root);
    fs::write(
        root.join("supertask.yaml"),
        "version: 1\nservices:\n  api:\n    kind: spring-boot\n    build_tool: gradle\n    module: user-service\n    port: 8091\n",
    )
    .unwrap();

    let eng = Engine::new();
    let (warnings, _) = eng.open(&root).expect("open");
    assert!(
        !warnings
            .iter()
            .any(|w| w.code == ErrorCode::BuildToolAmbiguous),
        "{warnings:?}"
    );
    eng.start_one("api").expect("start_one");
    assert!(
        wait_state(&eng, "api", RtState::Exited, Duration::from_secs(30)),
        "桩应很快退出"
    );
    let args = fs::read_to_string(root.join(".gradle-fake-args.txt")).expect("args file");
    assert_eq!(args.trim(), ":user-service:bootRun", "argv: {args:?}");
    eng.close().unwrap();
    let _ = fs::remove_dir_all(&root);
}

/// wrapper 与 PATH gradle 双缺失 → GRADLE_WRAPPER_MISSING（同步失败，不 spawn）。
#[test]
fn start_fails_with_gradle_wrapper_missing() {
    if find_on_path("gradle").is_some() {
        eprintln!("skip: PATH 中存在 gradle，无法模拟双缺失");
        return;
    }
    if !java_available() {
        eprintln!("skip: java 不在 PATH");
        return;
    }
    let root = ws_dir("missing");
    fs::create_dir_all(root.join("user-service")).unwrap();
    fs::write(
        root.join("user-service/build.gradle"),
        "plugins { id 'org.springframework.boot' }\n",
    )
    .unwrap();
    fs::write(
        root.join("supertask.yaml"),
        "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: user-service\n    port: 8092\n",
    )
    .unwrap();

    let eng = Engine::new();
    eng.open(&root).expect("open");
    let e = eng.start_one("api").unwrap_err();
    assert_eq!(e.code(), ErrorCode::GradleWrapperMissing);
    eng.close().unwrap();
    let _ = fs::remove_dir_all(&root);
}

/// launch: jar + 桩非零退出 → BUILD_FAILED，building 收场回到 Stopped。
#[test]
fn bootjar_build_failure_maps_to_build_failed() {
    if !java_available() {
        eprintln!("skip: java 不在 PATH");
        return;
    }
    let root = ws_dir("bootjarfail");
    fs::write(root.join("settings.gradle"), "include 'user-service'\n").unwrap();
    fs::create_dir_all(root.join("user-service")).unwrap();
    fs::write(
        root.join("user-service/build.gradle"),
        "plugins { id 'org.springframework.boot' }\n",
    )
    .unwrap();
    #[cfg(windows)]
    fs::write(root.join("gradlew.bat"), "@echo off\r\nexit /B 3\r\n").unwrap();
    #[cfg(not(windows))]
    fs::write(root.join("gradlew"), "#!/bin/sh\nexit 3\n").unwrap();
    fs::write(
        root.join("supertask.yaml"),
        "version: 1\nservices:\n  api:\n    kind: spring-boot\n    build_tool: gradle\n    module: user-service\n    port: 8093\n    launch: jar\n",
    )
    .unwrap();

    let eng = Engine::new();
    eng.open(&root).expect("open");
    eng.start_one("api")
        .expect("start_one accepted（异步构建）");
    // 初始态就是 Stopped：等「构建失败收场」= Stopped + last_error（先经过 Building）
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut last = None;
    while Instant::now() < deadline {
        if let Ok(snap) = eng.snapshot() {
            if let Some(s) = snap.services.get("api") {
                last = Some((s.state, s.last_error.clone()));
                if s.state == RtState::Stopped && s.last_error.is_some() {
                    break;
                }
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let (state, err) = last.expect("snapshot");
    assert_eq!(state, RtState::Stopped, "构建失败应回到 Stopped");
    let err = err.unwrap_or_default();
    assert!(err.contains("BUILD_FAILED"), "last_error: {err}");
    assert!(
        err.contains("gradle bootJar"),
        "应标注 gradle 构建阶段: {err}"
    );
    eng.close().unwrap();
    let _ = fs::remove_dir_all(&root);
}
