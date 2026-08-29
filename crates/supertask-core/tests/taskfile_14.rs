//! 1.4 §7 Taskfile v3 一次性导入：preview 输出、apply 后 YAML 快照、YAML_CONFLICT 路径。
//! 集成测试只触达 core 公共 API（taskfile::preview/apply + Engine::save_form），不 spawn 外部 GUI。

use std::fs;
use std::path::{Path, PathBuf};

use supertask_core::error::ErrorCode;
use supertask_core::spec::to_yaml;
use supertask_core::taskfile;
use supertask_core::Engine;

const SUPER_TASK_YAML: &str = "version: 1\nroot: .\nservices:\n  web:\n    kind: unknown-kind-for-test\n    cmd: \"echo hi\"\nscripts:\n  bootstrap:\n    cmds: [\"echo old\"]\n";

/// 集成 fixture：覆盖映射表全部关键分支（插值 / internal / deps / id 冲突 /
/// 非默认 shell / includes / 全局 env 合并 / cmd 映射形式 silent 丢弃）。
fn taskfile_fixture() -> &'static str {
    "version: '3'\n\
     env:\n\
     \x20 CI: \"1\"\n\
     includes:\n\
     \x20 sub: ./sub\n\
     tasks:\n\
     \x20 bootstrap:\n\
     \x20   desc: 重名任务\n\
     \x20   cmds: [echo new]\n\
     \x20 deploy-web:\n\
     \x20   env:\n\
     \x20     TOKEN: $DEPLOY_TOKEN\n\
     \x20   cmds:\n\
     \x20     - 'echo {{.TARGET}}'\n\
     \x20 helper:\n\
     \x20   internal: true\n\
     \x20   cmds: [echo helper]\n\
     \x20 lint-all:\n\
     \x20   deps: [build]\n\
     \x20   platforms: [windows]\n\
     \x20   cmds:\n\
     \x20     - cmd: npm run lint\n\
     \x20       silent: true\n\
     \x20 pwsh-only:\n\
     \x20   shell: powershell\n\
     \x20   cmds: [Write-Host hi]\n"
}

fn make_workspace(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("st-taskfile-it-{tag}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("supertask.yaml"), SUPER_TASK_YAML).unwrap();
    fs::write(dir.join("Taskfile.yml"), taskfile_fixture()).unwrap();
    dir
}

fn script_id_of(items: &[taskfile::TaskfileImportItem], task: &str) -> Option<String> {
    items
        .iter()
        .find(|i| i.task == task)
        .map(|i| i.script_id.clone())
}

#[test]
fn preview_outputs_mapping_table() {
    let dir = make_workspace("preview");
    let spec = supertask_core::parse_yaml(SUPER_TASK_YAML).unwrap().0;
    let out = taskfile::preview(&dir, Some(&spec.scripts)).unwrap();

    // pwsh-only（非默认 shell）被跳过；其余 4 项按 Taskfile 顺序出现
    let ids: Vec<&str> = out.tasks.iter().map(|t| t.script_id.as_str()).collect();
    assert_eq!(ids, vec!["bootstrap", "deploy-web", "helper", "lint-all"]);

    // id 冲突：目标已有 bootstrap → 默认 keep
    let bootstrap = &out.tasks[0];
    assert!(bootstrap.id_conflict && !bootstrap.selected);
    assert!(bootstrap.warnings.iter().any(|w| w.contains("同名脚本 id")));

    // 插值默认不勾选，警告列出变量
    let deploy = &out.tasks[1];
    assert!(!deploy.selected);
    let w = deploy.warnings.join("\n");
    assert!(w.contains("DEPLOY_TOKEN") && w.contains("TARGET"), "{w}");

    // internal 标灰
    let helper = &out.tasks[2];
    assert!(helper.internal && !helper.selected);

    // deps / platforms 警告；映射形式 cmd 保留、silent 丢弃
    let lint = &out.tasks[3];
    assert!(lint.selected && lint.cmds_count == 1);
    let w = lint.warnings.join("\n");
    assert!(w.contains("deps") && w.contains("platforms"), "{w}");

    // includes 全局警告
    assert!(out.warnings.iter().any(|w| w.contains("includes")));
}

#[test]
fn apply_produces_yaml_snapshot() {
    let dir = make_workspace("apply");
    let current = supertask_core::parse_yaml(SUPER_TASK_YAML).unwrap().0;
    let (merged, warnings) = taskfile::apply(
        &current,
        &dir,
        &[
            script_id_of(&preview_items(&dir), "deploy-web").unwrap(),
            "lint-all".to_string(),
        ],
    )
    .unwrap();

    // 只增改所选 scripts.*：既有 bootstrap 未被覆盖
    assert_eq!(
        merged.scripts.get("bootstrap").unwrap().cmds,
        vec!["echo old"]
    );
    // 插值按原文导入；全局 env CI 合并
    let deploy = merged.scripts.get("deploy-web").unwrap();
    assert_eq!(deploy.cmds, vec!["echo {{.TARGET}}"]);
    assert_eq!(
        deploy.env.get("TOKEN").map(String::as_str),
        Some("$DEPLOY_TOKEN")
    );
    assert_eq!(deploy.env.get("CI").map(String::as_str), Some("1"));
    // silent 丢弃
    assert_eq!(
        merged.scripts.get("lint-all").unwrap().cmds,
        vec!["npm run lint"]
    );

    let yaml = to_yaml(&merged).unwrap();
    assert!(yaml.contains("deploy-web:"), "snapshot:\n{yaml}");
    assert!(yaml.contains("TOKEN: $DEPLOY_TOKEN"), "snapshot:\n{yaml}");
    assert!(yaml.contains("CI: '1'"), "snapshot:\n{yaml}");
    assert!(yaml.contains("npm run lint"), "snapshot:\n{yaml}");
    assert!(warnings.iter().any(|w| w.contains("已导入 2 个脚本")));
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn engine_save_form_yaml_conflict_then_success() {
    let dir = make_workspace("conflict");
    let engine = Engine::new();
    engine.open(&dir).expect("open workspace");

    let current = engine.spec().unwrap();
    let items = taskfile::preview(&dir, Some(&current.scripts))
        .unwrap()
        .tasks;
    let selected = vec![script_id_of(&items, "lint-all").unwrap()];
    let (merged, _) = taskfile::apply(&current, &dir, &selected).unwrap();

    // 错误 base_hash → YAML_CONFLICT
    let err = engine.save_form(&merged, &"0".repeat(64)).unwrap_err();
    assert_eq!(err.code(), ErrorCode::YamlConflict);

    // 正确 base_hash → 写回成功，scripts.* 已含导入项
    let base_hash = engine.yaml_get().unwrap().hash;
    let (spec, hash, _) = engine.save_form(&merged, &base_hash).unwrap();
    assert!(spec.scripts.contains_key("lint-all"));
    assert_ne!(hash, base_hash);
    assert!(engine.yaml_get().unwrap().text.contains("lint-all"));

    engine.close().unwrap();
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn missing_taskfile_maps_to_not_found() {
    let dir = std::env::temp_dir().join(format!("st-taskfile-it-none-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join("supertask.yaml"), SUPER_TASK_YAML).unwrap();
    let err = taskfile::preview(&dir, None).unwrap_err();
    assert_eq!(err.code(), ErrorCode::TaskfileNotFound);
    let _ = fs::remove_dir_all(&dir);
}

fn preview_items(dir: &Path) -> Vec<taskfile::TaskfileImportItem> {
    taskfile::preview(dir, None).unwrap().tasks
}
