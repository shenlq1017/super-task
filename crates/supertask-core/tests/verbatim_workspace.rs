//! workspace_id / root 路径必须是干净的普通形式：
//! Windows 上 `std::fs::canonicalize` 会产生 verbatim 前缀（\\?\C:\…），
//! 引擎与 IPC 层都必须剥离后再作为 workspace_id 使用。

use supertask_core::Engine;

#[test]
fn workspace_id_has_no_verbatim_prefix() {
    let dir = std::env::temp_dir().join(format!("st-verbatim-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("supertask.yaml"),
        "version: 1\nroot: .\nservices:\n  web:\n    kind: unknown-kind-for-test\n    cmd: \"echo hi\"\n",
    )
    .unwrap();

    // 预先经 canonicalize 拿到 verbatim 形式，模拟外部传入的机器路径
    let verbatim = std::fs::canonicalize(&dir).unwrap();

    let engine = Engine::new();
    engine.open(&verbatim).expect("open should succeed");

    let id = engine.workspace_id().expect("workspace_id after open");
    assert!(
        !id.contains(r"\\?\"),
        "workspace_id 仍带 verbatim 前缀: {id}"
    );
    assert!(
        id.to_lowercase() == dir.to_string_lossy().to_lowercase() || {
            // temp 在 Windows 上可能是短路径/软链接规范化后的差异，退一步只断言盘符形态
            id.as_bytes().get(1) == Some(&b':')
        }
    );

    engine.close().expect("close");
    let _ = std::fs::remove_dir_all(&dir);
}
