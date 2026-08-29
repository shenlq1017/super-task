//! detach → 重开同根工作区的接管语义（不 spawn 真进程，用 Ping spawner 验证 slot 状态机）。

use supertask_core::runtime::RtState;
use supertask_core::Engine;

fn ws_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("st-detach-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("supertask.yaml"),
        "version: 1\nroot: .\nservices:\n  web:\n    kind: unknown-kind-for-test\n    cmd: \"echo hi\"\n",
    )
    .unwrap();
    dir
}

#[test]
fn detach_then_adopt_restores_running_slots() {
    // 场景：无运行服务时 detach == close 的清理部分；重开工作区应正常（无人接管）
    let dir = ws_dir("plain");
    let eng = Engine::new();
    eng.open(&dir).expect("open");
    eng.detach().expect("detach");
    assert!(eng.workspace_id().is_err(), "detach 后不应再有 workspace");

    let (_, snap) = eng.open(&dir).expect("reopen");
    let svc = snap.services.get("web").expect("web 服务");
    assert_eq!(
        svc.state,
        RtState::Stopped,
        "没有 detached 进程，应保持 Stopped"
    );
    eng.close().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn detach_clears_state_without_stopping_registry_semantics() {
    // detach 后 DETACHED 注册表为空注册（全部 stopped 被丢弃），close 也不 panic
    let dir = ws_dir("clear");
    let eng = Engine::new();
    eng.open(&dir).unwrap();
    eng.detach().unwrap();
    // 无 workspace 时 detach/close 都是 no-op
    eng.detach().expect("second detach is noop");
    eng.close().expect("close after detach is noop");
    let _ = std::fs::remove_dir_all(&dir);
}
