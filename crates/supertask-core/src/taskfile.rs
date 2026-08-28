//! 1.4 §7 Taskfile v3 一次性导入。文本级 YAML 子集解析，不引第三方 Taskfile 解析器。
//! 契约：`docs/spec/ipc.md` §10.8、`docs/plans/2026-08-28-v1-4-feature-spec.md` §7。
//!
//! 映射规则（§7.1）：task 名 → script id（合法化，导入内冲突加 `-task` 后缀）、
//! `desc` → `desc`、`cmds` → `cmds`（映射形式取 `cmd`，`silent` 丢弃）、`env` → `env`、
//! `dir` → `cwd`（沙箱校验，逃逸跳过）、`internal: true` 跳过（预览标灰）、
//! `deps`/`sources`/`generates`/`method`/`status`/`platforms` 忽略并警告、
//! 非默认 shell 跳过、`{{…}}`/`$VAR` 插值默认不勾选、includes/动态 task/loop 跳过。
//! 全局 `env` 合并进每 task（task 覆盖全局）；全局 `vars` 不解析。
//! 预览是纯内存计算；Apply 只增改所选 `scripts.*`，其余字段不动。

use std::collections::BTreeSet;
use std::path::Path;

use indexmap::IndexMap;
use serde::Serialize;
use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::is_valid_id;
use crate::spec::{ScriptSpec, SuperTaskFile};

/// §7.1 候选文件（工作区根，不递归），按优先级降序。
pub const TASKFILE_CANDIDATES: &[&str] = &["Taskfile.yml", "Taskfile.yaml"];

/// id 最长字符数（与 `ipc::is_valid_id` 上限一致）。
const MAX_ID_CHARS: usize = 64;

/// §7.2 `TaskfileImportItem`。`internal` / `id_conflict` 为 UI 展示扩展字段：
/// internal 行标灰不可选，id_conflict 行默认不勾（保留现有脚本）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TaskfileImportItem {
    /// Taskfile 原名。
    pub task: String,
    /// 目标 script id（已合法化）。
    pub script_id: String,
    pub cmds_count: usize,
    /// 默认动作（默认导入=true）。
    pub selected: bool,
    /// 该项的忽略/风险说明。
    pub warnings: Vec<String>,
    /// Taskfile `internal: true`；不可导入，预览标灰。
    pub internal: bool,
    /// 目标 `scripts.*` 已有同名 id；默认 keep。
    pub id_conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TaskfilePreview {
    pub tasks: Vec<TaskfileImportItem>,
    pub warnings: Vec<String>,
}

/// 预览中间产物：public item + 可导入的 ScriptSpec（internal/跳过项为 None）。
struct BuiltTask {
    item: TaskfileImportItem,
    spec: Option<ScriptSpec>,
}

/// 读工作区根 Taskfile（不递归、不跟 includes）。缺失 → `TASKFILE_NOT_FOUND`。
fn load_taskfile(root: &Path) -> Result<String> {
    let candidate = TASKFILE_CANDIDATES
        .iter()
        .find(|c| root.join(c).is_file())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::TaskfileNotFound,
                format!(
                    "工作区根目录未找到 {}（仅支持 Taskfile v3）",
                    TASKFILE_CANDIDATES.join(" / ")
                ),
            )
        })?;
    std::fs::read_to_string(root.join(candidate))
        .map_err(|e| Error::new(ErrorCode::TaskfileInvalid, format!("读取 Taskfile 失败: {e}")))
}

fn parse_yaml_value(text: &str) -> Result<Value> {
    let stripped = text.strip_prefix('\u{feff}').unwrap_or(text);
    serde_yaml::from_str(stripped).map_err(|e| {
        let mut err = Error::new(
            ErrorCode::TaskfileInvalid,
            format!("Taskfile YAML 解析失败: {e}"),
        );
        if let Some(line) = e.location().map(|l| l.line()) {
            err = err.details(serde_yaml::to_value(line).unwrap_or(Value::Null));
        }
        err
    })
}

/// §7.2 `import.taskfilePreview`：纯内存计算，无落盘。
/// `current_scripts` 来自当前 supertask.yaml，用于标记 id_conflict。
pub fn preview(root: &Path, current_scripts: Option<&IndexMap<String, ScriptSpec>>) -> Result<TaskfilePreview> {
    let (tasks, warnings) = build_items(root, current_scripts)?;
    Ok(TaskfilePreview {
        tasks: tasks.into_iter().map(|b| b.item).collect(),
        warnings,
    })
}

/// §7.2 `import.taskfileApply`：按选择合并进 current，只增改所选 `scripts.*`。
/// 返回合并后的 spec 与导入警告；写回由调用方走 `yaml.saveForm`（base_hash 冲突 → `YAML_CONFLICT`）。
pub fn apply(
    current: &SuperTaskFile,
    root: &Path,
    selected: &[String],
) -> Result<(SuperTaskFile, Vec<String>)> {
    let (built, mut warnings) = build_items(root, Some(&current.scripts))?;
    let mut out = current.clone();
    let mut missing: Vec<&String> = Vec::new();
    let mut applied = 0usize;
    for item in &built {
        if !selected.iter().any(|s| s == &item.item.script_id) {
            continue;
        }
        if item.item.internal || item.spec.is_none() {
            warnings.push(format!("任务 {} 不可导入，已跳过", item.item.task));
            continue;
        }
        let spec = item.spec.as_ref().expect("非 internal 项必有 spec");
        if item.item.id_conflict {
            warnings.push(format!(
                "覆盖目标已有脚本 {}（用户显式选择）",
                item.item.script_id
            ));
        }
        out.scripts
            .insert(item.item.script_id.clone(), spec.clone());
        applied += 1;
    }
    for s in selected {
        if !built.iter().any(|b| &b.item.script_id == s) {
            missing.push(s);
        }
    }
    if !missing.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("预览项不存在: {}", missing.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")),
        ));
    }
    warnings.push(format!("已导入 {applied} 个脚本（一次性迁移，之后不跟随 Taskfile 变化）"));
    Ok((out, warnings))
}

fn build_items(
    root: &Path,
    current_scripts: Option<&IndexMap<String, ScriptSpec>>,
) -> Result<(Vec<BuiltTask>, Vec<String>)> {
    let text = load_taskfile(root)?;
    let value = parse_yaml_value(&text)?;
    let map = value.as_mapping().ok_or_else(|| {
        Error::new(ErrorCode::TaskfileInvalid, "Taskfile 顶层必须是映射")
    })?;

    // version 必须是 '3'（字符串或数字）
    match map.get(Value::from("version")) {
        Some(Value::String(s)) if s.trim() == "3" => {}
        Some(Value::Number(n)) if n.as_u64() == Some(3) => {}
        Some(Value::String(s)) if s.trim() == "2" || s.trim() == "2.x" => {
            return Err(Error::new(
                ErrorCode::TaskfileInvalid,
                "Taskfile v2 不支持，仅支持 v3",
            ));
        }
        Some(Value::Number(n)) if n.as_u64() == Some(2) => {
            return Err(Error::new(
                ErrorCode::TaskfileInvalid,
                "Taskfile v2 不支持，仅支持 v3",
            ));
        }
        _ => {
            return Err(Error::new(
                ErrorCode::TaskfileInvalid,
                "Taskfile version 缺失或不支持（仅支持 v3）",
            ));
        }
    }

    let mut warnings: Vec<String> = Vec::new();

    // includes / 全局 vars：不解析，只警告
    if let Some(inc) = map.get(Value::from("includes")) {
        let count = inc.as_mapping().map(|m| m.len()).unwrap_or(0);
        if count > 0 {
            warnings.push(format!(
                "includes 不支持且未跟随：{count} 个子 Taskfile 已跳过，需要的任务请手工补录"
            ));
        }
    }
    if map.get(Value::from("vars")).is_some_and(|v| !v.is_null()) {
        warnings.push("全局 vars 不解析，插值保持原文导入".to_string());
    }

    let global_env = parse_env(map.get(Value::from("env")), &mut warnings);

    let tasks_value = map.get(Value::from("tasks"));
    let empty_map = serde_yaml::Mapping::new();
    let tasks = match tasks_value {
        Some(Value::Mapping(m)) => m,
        Some(Value::Null) | None => &empty_map,
        _ => {
            return Err(Error::new(
                ErrorCode::TaskfileInvalid,
                "tasks 段必须是映射",
            ));
        }
    };

    // tasks 段的键顺序（serde_yaml Mapping 保序）
    let mut built: Vec<BuiltTask> = Vec::new();
    let mut used_ids: BTreeSet<String> = BTreeSet::new();
    for (key, value) in tasks {
        let Some(task_name) = key.as_str().map(str::to_string) else {
            warnings.push("tasks 存在非字符串任务名，已跳过".to_string());
            continue;
        };
        build_one_task(root, &task_name, value, &global_env, current_scripts, &mut used_ids, &mut built, &mut warnings);
    }
    Ok((built, warnings))
}

#[allow(clippy::too_many_arguments)]
fn build_one_task(
    root: &Path,
    task_name: &str,
    value: &Value,
    global_env: &IndexMap<String, String>,
    current_scripts: Option<&IndexMap<String, ScriptSpec>>,
    used_ids: &mut BTreeSet<String>,
    built: &mut Vec<BuiltTask>,
    warnings: &mut Vec<String>,
) {
    let skip = |reason: String, warnings: &mut Vec<String>| {
        warnings.push(format!("任务 {task_name} 跳过：{reason}"));
    };

    let Value::Mapping(task_map) = value else {
        skip("任务定义必须是映射".into(), warnings);
        return;
    };

    // internal: true → 预览标灰，不可导入
    let internal = task_map
        .get(Value::from("internal"))
        .is_some_and(|v| v == &Value::Bool(true));

    // 非默认 shell → 跳过（引擎统一 bash -c 执行）
    if let Some(shell) = task_map.get(Value::from("shell")) {
        let shell_name = shell.as_str().unwrap_or("");
        if !matches!(shell_name, "bash" | "sh") {
            skip(format!("指定了非默认 shell（{shell_name}），执行 shell 不一致"), warnings);
            return;
        }
    }

    // 任务级 loop → 跳过
    if task_map.get(Value::from("for")).is_some() {
        skip("loop（for）任务不支持".into(), warnings);
        return;
    }

    let mut item_warnings: Vec<String> = Vec::new();

    // 忽略字段警告（§7.1）
    if task_map.get(Value::from("deps")).is_some_and(|v| !v.is_null()) {
        item_warnings.push("deps 忽略（scripts.depends_on 预留）".to_string());
    }
    for field in ["sources", "generates", "method", "status"] {
        if task_map
            .get(Value::from(field))
            .is_some_and(|v| !v.is_null())
        {
            item_warnings.push(format!("{field} 忽略"));
        }
    }
    if task_map
        .get(Value::from("platforms"))
        .is_some_and(|v| !v.is_null())
    {
        item_warnings.push("platforms 约束忽略，导入后的脚本无平台限制".to_string());
    }

    // dir → cwd（沙箱校验，逃逸跳过该任务）
    let mut cwd: Option<String> = None;
    if let Some(dir) = task_map.get(Value::from("dir")) {
        if !dir.is_null() {
            let Some(dir_str) = dir.as_str() else {
                skip("dir 必须是字符串".into(), warnings);
                return;
            };
            let dir_str = dir_str.trim();
            if !dir_str.is_empty() && dir_str != "." {
                if let Err(e) = crate::sandbox::confine(root, dir_str) {
                    skip(format!("dir {dir_str} 逃出工作区（{e}），未导入"), warnings);
                    return;
                }
                cwd = Some(dir_str.to_string());
            }
        }
    }

    // cmds（字符串或 {cmd, silent} 映射）
    let mut cmds: Vec<String> = Vec::new();
    let mut interpolations: BTreeSet<String> = BTreeSet::new();
    match task_map.get(Value::from("cmds")) {
        Some(Value::Sequence(seq)) => {
            for entry in seq {
                match entry {
                    Value::String(s) => {
                        collect_interpolations(s, &mut interpolations);
                        cmds.push(s.clone());
                    }
                    Value::Mapping(m) => {
                        // 动态形式优先：loop（for）/ 引用任务（task:）跳过；
                        // 静态映射形式 { cmd: ..., silent: ... } 只取 cmd，silent 丢弃
                        if m.get(Value::from("for")).is_some() {
                            item_warnings.push("loop（for）命令跳过".to_string());
                        } else if m.get(Value::from("task")).is_some() {
                            item_warnings.push("引用任务的命令（task:）跳过".to_string());
                        } else if let Some(Value::String(cmd)) = m.get(Value::from("cmd")) {
                            collect_interpolations(cmd, &mut interpolations);
                            cmds.push(cmd.clone());
                        } else {
                            item_warnings.push("无 cmd 的命令项跳过".to_string());
                        }
                    }
                    _ => item_warnings.push("无法识别的命令项跳过".to_string()),
                }
            }
        }
        Some(Value::Null) | None => {}
        _ => {
            skip("cmds 必须是列表".into(), warnings);
            return;
        }
    }
    if cmds.is_empty() {
        skip("无静态 cmds".into(), warnings);
        return;
    }

    // env：task 覆盖全局
    let mut env: IndexMap<String, String> = global_env.clone();
    let task_env = parse_env(task_map.get(Value::from("env")), &mut item_warnings);
    for (k, v) in task_env {
        env.insert(k, v);
    }
    for v in env.values() {
        collect_interpolations(v, &mut interpolations);
    }

    // id 合法化 + 导入内冲突 → -task 后缀
    let script_id = unique_id(legalize_id(task_name), used_ids);
    if script_id != task_name {
        item_warnings.insert(
            0,
            format!("任务名 {task_name} 合法化为脚本 id {script_id}"),
        );
    }

    let id_conflict = current_scripts
        .map(|scripts| scripts.contains_key(&script_id))
        .unwrap_or(false);
    if id_conflict {
        item_warnings.push("目标已存在同名脚本 id，默认保留现有脚本；勾选将覆盖".to_string());
    }

    if internal {
        item_warnings.push("internal 任务不导入".to_string());
    }

    let mut selected = true;
    if internal {
        selected = false;
    }
    if id_conflict {
        selected = false;
    }
    if !interpolations.is_empty() {
        selected = false;
        item_warnings.push(format!(
            "包含插值变量 {}，未解析；勾选后按原文导入",
            interpolations.into_iter().collect::<Vec<_>>().join(", ")
        ));
    }

    let spec = if internal {
        None
    } else {
        Some(ScriptSpec {
            desc: task_map
                .get(Value::from("desc"))
                .and_then(Value::as_str)
                .map(str::to_string),
            cmds,
            cwd,
            env,
            timeout_secs: None,
            depends_on: Vec::new(),
        })
    };

    built.push(BuiltTask {
        item: TaskfileImportItem {
            task: task_name.to_string(),
            cmds_count: spec.as_ref().map(|s| s.cmds.len()).unwrap_or(0),
            script_id,
            selected,
            warnings: item_warnings,
            internal,
            id_conflict,
        },
        spec,
    });
}

/// env 段（全局或 task 级）解析：只取标量值，非标量跳过并警告。
fn parse_env(value: Option<&Value>, warnings: &mut Vec<String>) -> IndexMap<String, String> {
    let mut out = IndexMap::new();
    let Some(Value::Mapping(m)) = value else {
        return out;
    };
    for (k, v) in m {
        let Some(key) = k.as_str() else {
            warnings.push("env 存在非字符串键，已跳过".to_string());
            continue;
        };
        if v.is_null() {
            out.insert(key.to_string(), String::new());
            continue;
        }
        match scalar_to_string(v) {
            Some(s) => {
                out.insert(key.to_string(), s);
            }
            None => warnings.push(format!("env {key} 不是标量值，已跳过")),
        }
    }
    out
}

fn scalar_to_string(v: &Value) -> Option<String> {
    match v {
        Value::Bool(b) => Some(b.to_string()),
        Value::Number(n) => Some(n.to_string()),
        Value::String(s) => Some(s.clone()),
        _ => None,
    }
}

/// 按 id 规则（`^[A-Za-z][A-Za-z0-9_-]{0,63}$`）合法化：
/// 非法字符替换为 `-`；首字符非字母补 `task-` 前缀；超长截断。
fn legalize_id(raw: &str) -> String {
    let mut s: String = raw
        .trim()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if s.chars().next().is_none_or(|c| !c.is_ascii_alphabetic()) {
        s.insert_str(0, "task-");
    }
    if s.len() > MAX_ID_CHARS {
        s.truncate(MAX_ID_CHARS);
    }
    if !is_valid_id(&s) {
        s = "task".to_string();
    }
    s
}

/// 导入内 id 冲突 → `-task` 后缀（仍冲突则追加序号）。
fn unique_id(candidate: String, used: &mut BTreeSet<String>) -> String {
    let mut id = candidate.clone();
    if used.contains(&id) {
        id = format!("{candidate}-task");
        let mut n = 2;
        while used.contains(&id) {
            id = format!("{candidate}-task-{n}");
            n += 1;
        }
    }
    used.insert(id.clone());
    id
}

/// 文本级插值检测：`{{…}}`（Go template）与 `$VAR` / `${VAR}`。
fn collect_interpolations(text: &str, out: &mut BTreeSet<String>) {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        // {{ … }}
        if bytes[i] == b'{' && i + 1 < bytes.len() && bytes[i + 1] == b'{' {
            if let Some(end) = text[i + 2..].find("}}") {
                let inner = text[i + 2..i + 2 + end].trim();
                let name = template_var_name(inner);
                if !name.is_empty() {
                    out.insert(name);
                }
                i += 2 + end + 2;
                continue;
            }
            i += 2;
            continue;
        }
        if bytes[i] == b'$' && i + 1 < bytes.len() {
            let next = bytes[i + 1];
            if next == b'$' {
                // $$ 转义
                i += 2;
                continue;
            }
            if next == b'{' {
                if let Some(end) = text[i + 2..].find('}') {
                    let inner = text[i + 2..i + 2 + end].trim();
                    if !inner.is_empty() {
                        out.insert(inner.to_string());
                    }
                    i += 2 + end + 1;
                    continue;
                }
                i += 2;
                continue;
            }
            if next.is_ascii_alphabetic() || next == b'_' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                out.insert(text[i + 1..j].to_string());
                i = j;
                continue;
            }
        }
        i += 1;
    }
}

/// `{{.VAR}}` / `{{ .VAR | default "x" }}` → 变量名；解析不出时保留原文。
fn template_var_name(inner: &str) -> String {
    let stripped = inner.trim().trim_start_matches('.');
    let name: String = stripped
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect();
    if name.is_empty() {
        inner.to_string()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_ws(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "st-taskfile-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_taskfile(tag: &str, text: &str) -> PathBuf {
        let dir = temp_ws(tag);
        fs::write(dir.join("Taskfile.yml"), text).unwrap();
        dir
    }

    fn preview_tasks(dir: &Path) -> Vec<TaskfileImportItem> {
        preview(dir, None).unwrap().tasks
    }

    #[test]
    fn missing_file_maps_to_taskfile_not_found() {
        let dir = temp_ws("missing");
        let err = preview(&dir, None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::TaskfileNotFound);
    }

    #[test]
    fn bad_yaml_is_invalid_with_line() {
        let dir = write_taskfile("badyaml", "version: '3'\ntasks:\n  a: [unclosed\n");
        let err = preview(&dir, None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::TaskfileInvalid);
    }

    #[test]
    fn v2_is_rejected() {
        for text in [
            "version: '2'\ntasks:\n  a:\n    cmds: [echo hi]\n",
            "version: 2\ntasks:\n  a:\n    cmds: [echo hi]\n",
        ] {
            let dir = write_taskfile("v2", text);
            let err = preview(&dir, None).unwrap_err();
            assert_eq!(err.code(), ErrorCode::TaskfileInvalid);
            assert!(err.message().contains("v2"), "{}", err.message());
        }
    }

    #[test]
    fn version_three_accepts_string_and_number() {
        for text in [
            "version: '3'\ntasks:\n  a:\n    cmds: [echo hi]\n",
            "version: 3\ntasks:\n  a:\n    cmds: [echo hi]\n",
        ] {
            let dir = write_taskfile("v3", text);
            let tasks = preview_tasks(&dir);
            assert_eq!(tasks.len(), 1);
            assert_eq!(tasks[0].script_id, "a");
            assert!(tasks[0].selected);
        }
    }

    #[test]
    fn desc_cmds_and_cmd_map_form_mapping() {
        let dir = write_taskfile(
            "mapping",
            "version: '3'\ntasks:\n  build:\n    desc: 构建前端\n    cmds:\n      - npm run build\n      - cmd: npm run lint\n        silent: true\n",
        );
        let tasks = preview_tasks(&dir);
        assert_eq!(tasks.len(), 1);
        let item = &tasks[0];
        assert_eq!(item.script_id, "build");
        assert_eq!(item.cmds_count, 2);
        assert!(item.selected);
        assert!(item.warnings.is_empty());

        let merged = merged_spec(&demo_spec(), &dir, &["build"]);
        let script = merged.scripts.get("build").unwrap();
        assert_eq!(script.desc.as_deref(), Some("构建前端"));
        assert_eq!(script.cmds, vec!["npm run build", "npm run lint"]);
    }

    #[test]
    fn internal_task_is_greyed_and_not_imported() {
        let dir = write_taskfile(
            "internal",
            "version: '3'\ntasks:\n  helper:\n    internal: true\n    cmds: [echo helper]\n",
        );
        let tasks = preview_tasks(&dir);
        assert_eq!(tasks.len(), 1);
        assert!(tasks[0].internal && !tasks[0].selected);
        assert!(tasks[0].warnings.iter().any(|w| w.contains("internal")));
        let merged = merged_spec(&demo_spec(), &dir, &["helper"]);
        assert!(merged.scripts.get("helper").is_none(), "internal 不可被选中导入");
    }

    #[test]
    fn interpolation_defaults_unselected_but_forces_raw() {
        let dir = write_taskfile(
            "interp",
            "version: '3'\ntasks:\n  deploy:\n    env:\n      TOKEN: $DEPLOY_TOKEN\n    cmds:\n      - echo {{.TARGET}}\n      - 'curl -H \"X-Key: ${API_KEY}\" https://x'\n",
        );
        let tasks = preview_tasks(&dir);
        let item = &tasks[0];
        assert!(!item.selected, "插值默认不勾选");
        let w = item.warnings.join("\n");
        assert!(w.contains("TARGET"), "{w}");
        assert!(w.contains("API_KEY"), "{w}");
        assert!(w.contains("DEPLOY_TOKEN"), "{w}");
        // 强制导入 → 原文
        let merged = merged_spec(&demo_spec(), &dir, &["deploy"]);
        let script = merged.scripts.get("deploy").unwrap();
        assert_eq!(script.cmds[0], "echo {{.TARGET}}");
        assert_eq!(script.env.get("TOKEN").map(String::as_str), Some("$DEPLOY_TOKEN"));
    }

    #[test]
    fn id_legalization_and_task_suffix_conflict() {
        let dir = write_taskfile(
            "legalize",
            "version: '3'\ntasks:\n  build.web:\n    cmds: [echo w]\n  build-web:\n    cmds: [echo w2]\n  1abc:\n    cmds: [echo n]\n",
        );
        let tasks = preview_tasks(&dir);
        assert_eq!(tasks[0].script_id, "build-web");
        // build.web → build-web 与 build_web 合法化结果冲突 → -task 后缀
        assert_eq!(tasks[1].script_id, "build-web-task");
        assert!(tasks[1]
            .warnings
            .iter()
            .any(|w| w.contains("-task") || w.contains("合法化")));
        // 首字符非字母 → task- 前缀
        assert_eq!(tasks[2].script_id, "task-1abc");
    }

    #[test]
    fn global_env_merges_with_task_override() {
        let dir = write_taskfile(
            "env",
            "version: '3'\nenv:\n  GLOBAL: g\n  SHARED: from-global\ntasks:\n  t:\n    env:\n      SHARED: from-task\n      ONLY: 1\n    cmds: [echo hi]\n",
        );
        let merged = merged_spec(&demo_spec(), &dir, &["t"]);
        let env = &merged.scripts.get("t").unwrap().env;
        assert_eq!(env.get("GLOBAL").map(String::as_str), Some("g"));
        assert_eq!(env.get("SHARED").map(String::as_str), Some("from-task"));
        assert_eq!(env.get("ONLY").map(String::as_str), Some("1"));
    }

    #[test]
    fn cwd_escape_skips_task() {
        let dir = write_taskfile(
            "escape",
            "version: '3'\ntasks:\n  out:\n    dir: ../outside\n    cmds: [echo hi]\n  ok:\n    dir: ./web\n    cmds: [echo hi]\n",
        );
        let tasks = preview_tasks(&dir);
        let ids: Vec<&str> = tasks.iter().map(|t| t.script_id.as_str()).collect();
        assert!(!ids.contains(&"out"), "逃逸任务不出现在预览: {ids:?}");
        assert!(ids.contains(&"ok"));
        let merged = merged_spec(&demo_spec(), &dir, &["ok"]);
        assert_eq!(
            merged.scripts.get("ok").unwrap().cwd.as_deref(),
            Some("./web")
        );
    }

    #[test]
    fn ignored_fields_warn_and_import_continues() {
        let dir = write_taskfile(
            "ignored",
            "version: '3'\nincludes:\n  sub: ./sub\ntasks:\n  t:\n    deps: [other]\n    sources: [src]\n    generates: [out]\n    method: none\n    status: [up]\n    platforms: [windows]\n    cmds: [echo hi]\n",
        );
        let out = preview(&dir, None).unwrap();
        assert!(out.warnings.iter().any(|w| w.contains("includes")));
        let item = &out.tasks[0];
        let w = item.warnings.join("\n");
        for token in ["deps", "sources", "generates", "method", "status", "platforms"] {
            assert!(w.contains(token), "{token} 缺警告: {w}");
        }
        assert!(item.selected);
    }

    #[test]
    fn non_default_shell_skips_task() {
        let dir = write_taskfile(
            "shell",
            "version: '3'\ntasks:\n  pwsh:\n    shell: powershell\n    cmds: [Write-Host hi]\n  sh:\n    shell: bash\n    cmds: [echo hi]\n",
        );
        let tasks = preview_tasks(&dir);
        let ids: Vec<&str> = tasks.iter().map(|t| t.script_id.as_str()).collect();
        assert_eq!(ids, vec!["sh"]);
    }

    #[test]
    fn task_ref_and_loop_cmds_are_skipped() {
        let dir = write_taskfile(
            "dynamic",
            "version: '3'\ntasks:\n  caller:\n    cmds:\n      - task: build\n      - cmd: echo ok\n      - for: src/*.go\n        cmd: echo {{.ITEM}}\n",
        );
        let tasks = preview_tasks(&dir);
        assert_eq!(tasks.len(), 1);
        assert_eq!(tasks[0].cmds_count, 1, "只有静态 cmd 保留");
        assert!(tasks[0].warnings.iter().any(|w| w.contains("task:")));
        assert!(tasks[0].warnings.iter().any(|w| w.contains("for")));
    }

    #[test]
    fn id_conflict_with_existing_script_defaults_keep() {
        let dir = write_taskfile(
            "conflict",
            "version: '3'\ntasks:\n  bootstrap:\n    cmds: [echo new]\n",
        );
        let mut current = demo_spec();
        current.scripts.insert(
            "bootstrap".into(),
            ScriptSpec {
                desc: None,
                cmds: vec!["echo old".into()],
                cwd: None,
                env: IndexMap::new(),
                timeout_secs: None,
                depends_on: Vec::new(),
            },
        );
        let out = preview(&dir, Some(&current.scripts)).unwrap();
        let item = &out.tasks[0];
        assert!(item.id_conflict && !item.selected);
        // 默认不选 → apply 不带它，旧脚本保留
        let (merged, _) = apply(&current, &dir, &[]).unwrap();
        assert_eq!(
            merged.scripts.get("bootstrap").unwrap().cmds,
            vec!["echo old"]
        );
        // 显式选择 → 覆盖
        let (merged, warnings) = apply(&current, &dir, &["bootstrap".to_string()]).unwrap();
        assert_eq!(
            merged.scripts.get("bootstrap").unwrap().cmds,
            vec!["echo new"]
        );
        assert!(warnings.iter().any(|w| w.contains("覆盖")));
    }

    #[test]
    fn apply_rejects_unknown_selected_id() {
        let dir = write_taskfile(
            "unknown",
            "version: '3'\ntasks:\n  a:\n    cmds: [echo hi]\n",
        );
        let err = apply(&demo_spec(), &dir, &["nope".to_string()]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
    }

    #[test]
    fn taskfile_yaml_ext_is_found() {
        let dir = temp_ws("yaml-ext");
        fs::write(dir.join("Taskfile.yaml"), "version: '3'\ntasks:\n  a:\n    cmds: [echo hi]\n").unwrap();
        let tasks = preview_tasks(&dir);
        assert_eq!(tasks.len(), 1);
    }

    #[test]
    fn global_vars_warn_only() {
        let dir = write_taskfile(
            "vars",
            "version: '3'\nvars:\n  NAME: demo\ntasks:\n  a:\n    cmds: ['echo {{.NAME}}']\n",
        );
        let out = preview(&dir, None).unwrap();
        assert!(out.warnings.iter().any(|w| w.contains("vars")));
        let item = &out.tasks[0];
        assert!(!item.selected);
    }

    // ---- helpers ----

    fn demo_spec() -> SuperTaskFile {
        serde_yaml::from_str(
            "version: 1\nroot: .\nservices:\n  web:\n    kind: node\n    script: dev\n",
        )
        .unwrap()
    }

    fn merged_spec(current: &SuperTaskFile, dir: &Path, selected: &[&str]) -> SuperTaskFile {
        let sel: Vec<String> = selected.iter().map(|s| s.to_string()).collect();
        apply(current, dir, &sel).unwrap().0
    }
}
