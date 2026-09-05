//! 方向二：Procfile（Foreman / Overmind / Heroku 生态）一次性导入。
//! 契约：`docs/spec/ipc.md` §10.19；格式极简——每行 `name: command`。
//!
//! 映射规则：name → 服务 id（合法化，导入内冲突加 `-proc` 后缀）；command 忠实拆为
//! `kind: generic` 的 `program` + `args`（sh 风格引号感知切词）。含 shell 语法
//! （`$`、管道/重定向/组合操作符、反引号、通配符等）的命令**跳过不导入**——generic
//! 不经 shell 执行，变量插值与操作符无法忠实表达（与 Taskfile「按原文导入」不同，
//! 那里脚本走 `bash -c`，这里没有等价落点）。`.env`（Foreman 约定自动加载）存在时
//! 草稿挂 `env_file: [.env]` 引用，**值不内联、不回显**。
//! 预览是纯内存计算；Apply 只增改所选 `services.*`，其余字段不动。

use std::collections::BTreeSet;
use std::path::Path;

use indexmap::IndexMap;
use serde::Serialize;

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::is_valid_id;
use crate::spec::{ServiceSpec, SuperTaskFile};

/// id 最长字符数（与 `ipc::is_valid_id` 上限一致）。
const MAX_ID_CHARS: usize = 64;

/// Foreman 约定：命令依赖的本地环境变量文件（存在则挂 env_file，不内联）。
const DOTENV: &str = ".env";

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProcfileImportItem {
    /// Procfile 原名。
    pub name: String,
    /// 目标服务 id（已合法化）。
    pub service_id: String,
    /// 原命令行（拆词前原文）。
    pub command: String,
    /// 默认动作（默认导入=true；跳过项恒 false 且不可导入）。
    pub selected: bool,
    /// 该项的忽略/风险说明。
    pub warnings: Vec<String>,
    /// 含 shell 语法无法忠实导入；预览标灰。
    pub skipped: bool,
    /// 目标 `services.*` 已有同名 id；默认 keep。
    pub id_conflict: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProcfilePreview {
    pub items: Vec<ProcfileImportItem>,
    pub warnings: Vec<String>,
}

/// 预览中间产物：public item + 可导入的 ServiceSpec（skipped 项为 None）。
struct BuiltEntry {
    item: ProcfileImportItem,
    spec: Option<ServiceSpec>,
}

/// 读工作区根 Procfile（不递归）。缺失 → `PROCFILE_NOT_FOUND`。
fn load_procfile(root: &Path) -> Result<String> {
    let path = root.join("Procfile");
    if !path.is_file() {
        return Err(Error::new(
            ErrorCode::ProcfileNotFound,
            "工作区根目录未找到 Procfile（Foreman / Overmind 格式：每行 `name: command`）",
        ));
    }
    std::fs::read_to_string(&path).map_err(|e| {
        Error::new(
            ErrorCode::ProcfileInvalid,
            format!("读取 Procfile 失败: {e}"),
        )
    })
}

/// §10.19 `import.procfilePreview`：纯内存计算，无落盘。
/// `current_services` 来自当前 supertask.yaml，用于标记 id_conflict。
pub fn preview(
    root: &Path,
    current_services: Option<&IndexMap<String, ServiceSpec>>,
) -> Result<ProcfilePreview> {
    let (items, warnings) = build_entries(root, current_services)?;
    Ok(ProcfilePreview {
        items: items.into_iter().map(|b| b.item).collect(),
        warnings,
    })
}

/// §10.19 `import.procfileApply`：按选择合并进 current，只增改所选 `services.*`。
/// 返回合并后的 spec 与导入警告；写回由调用方走 `yaml.saveForm`（base_hash 冲突 → `YAML_CONFLICT`）。
pub fn apply(
    current: &SuperTaskFile,
    root: &Path,
    selected: &[String],
) -> Result<(SuperTaskFile, Vec<String>)> {
    let (built, mut warnings) = build_entries(root, Some(&current.services))?;
    let mut out = current.clone();
    let mut missing: Vec<&String> = Vec::new();
    let mut applied = 0usize;
    for entry in &built {
        if !selected.iter().any(|s| s == &entry.item.service_id) {
            continue;
        }
        if entry.item.skipped || entry.spec.is_none() {
            warnings.push(format!("条目 {} 不可导入，已跳过", entry.item.name));
            continue;
        }
        let spec = entry.spec.as_ref().expect("非 skipped 项必有 spec");
        if entry.item.id_conflict {
            warnings.push(format!(
                "覆盖目标已有服务 {}（用户显式选择）",
                entry.item.service_id
            ));
        }
        out.services
            .insert(entry.item.service_id.clone(), spec.clone());
        applied += 1;
    }
    for s in selected {
        if !built.iter().any(|b| &b.item.service_id == s) {
            missing.push(s);
        }
    }
    if !missing.is_empty() {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!(
                "预览项不存在: {}",
                missing
                    .iter()
                    .map(|s| s.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        ));
    }
    warnings.push(format!(
        "已导入 {applied} 个服务（一次性迁移，之后不跟随 Procfile 变化）"
    ));
    Ok((out, warnings))
}

fn build_entries(
    root: &Path,
    current_services: Option<&IndexMap<String, ServiceSpec>>,
) -> Result<(Vec<BuiltEntry>, Vec<String>)> {
    let text = load_procfile(root)?;
    let mut warnings: Vec<String> = Vec::new();

    // Foreman 约定：根目录 .env 自动注入进程环境 → env_file 引用（值不内联不回显）
    let has_dotenv = root.join(DOTENV).is_file();
    if has_dotenv {
        warnings.push(format!(
            "检测到 {DOTENV}：导入的服务通过 env_file 引用它（值不写入 yaml）；重命名或移动 .env 需同步调整"
        ));
    }

    let mut built: Vec<BuiltEntry> = Vec::new();
    let mut used_ids: BTreeSet<String> = BTreeSet::new();
    let mut seen_names: BTreeSet<String> = BTreeSet::new();
    let mut count = 0usize;
    for raw_line in text.lines() {
        let line = raw_line.trim();
        count += 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((name, command)) = split_entry(line) else {
            warnings.push(format!("第 {} 行不是 `name: command` 格式，已跳过", count));
            continue;
        };
        if !seen_names.insert(name.to_string()) {
            warnings.push(format!("条目 {name} 重复，仅保留首个定义"));
            continue;
        }
        build_one_entry(
            name,
            command,
            has_dotenv,
            current_services,
            &mut used_ids,
            &mut built,
            &mut warnings,
        );
    }
    if built.is_empty() {
        warnings.push("Procfile 中没有可识别条目".to_string());
    }
    Ok((built, warnings))
}

/// 按 Foreman 语义切 `name: command`：首个 `: ` 前为 name（`[\w.-]+`），其余为命令。
/// Foreman 允许 `name:cmd` 无空格（`:` 后即命令）——同样接受；命令为空视为无效行。
fn split_entry(line: &str) -> Option<(&str, &str)> {
    let colon = line.find(':')?;
    let name = &line[..colon];
    if name.is_empty()
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '-')
    {
        return None;
    }
    let command = line[colon + 1..].trim();
    if command.is_empty() {
        return None;
    }
    Some((name, command))
}

#[allow(clippy::too_many_arguments)]
fn build_one_entry(
    name: &str,
    command: &str,
    has_dotenv: bool,
    current_services: Option<&IndexMap<String, ServiceSpec>>,
    used_ids: &mut BTreeSet<String>,
    built: &mut Vec<BuiltEntry>,
    warnings: &mut Vec<String>,
) {
    let mut item_warnings: Vec<String> = Vec::new();

    // 跳过项不导入，id_conflict 无意义（恒 false）
    let mut skip = |item_warnings: Vec<String>| {
        let service_id = unique_id(legalize_id(name), used_ids);
        built.push(BuiltEntry {
            item: ProcfileImportItem {
                name: name.to_string(),
                service_id,
                command: command.to_string(),
                selected: false,
                warnings: item_warnings,
                skipped: true,
                id_conflict: false,
            },
            spec: None,
        });
    };

    // generic 不经 shell 执行：含 shell 语法的命令无法忠实表达 → 跳过不导入
    if let Some(ch) = find_shell_metachar(command) {
        item_warnings.push(format!(
            "命令含 shell 语法（{ch}），不经 shell 无法忠实表达；请在 supertask.yaml 手工配置（scripts 走 bash -c 可承接此类命令）"
        ));
        skip(item_warnings);
        return;
    }

    let Some(tokens) = tokenize(command) else {
        item_warnings.push("命令引号不配对或拆词为空，无法忠实导入".to_string());
        skip(item_warnings);
        return;
    };

    let spec = ServiceSpec {
        kind: "generic".into(),
        service: None,
        enabled: true,
        group: None,
        labels: IndexMap::from([
            ("origin".to_string(), "imported".to_string()),
            ("imported-from".to_string(), format!("Procfile:{name}")),
        ]),
        port: None,
        ports: Vec::new(),
        env: IndexMap::new(),
        env_file: if has_dotenv {
            vec![DOTENV.to_string()]
        } else {
            Vec::new()
        },
        depends_on: Vec::new(),
        depends_on_ex: None,
        grace_secs: None,
        health: None,
        restart: None,
        max_retries: None,
        extra_args: Vec::new(),
        build_args: Vec::new(),
        cwd: None,
        launch: None,
        module: None,
        build_tool: None,
        jvm_args: Vec::new(),
        dir: None,
        package_manager: None,
        script: None,
        entry: None,
        package: None,
        program: Some(tokens[0].clone()),
        args: tokens[1..].to_vec(),
        logging: None,
        resources: None,
        extra: IndexMap::new(),
    };

    let service_id = unique_id(legalize_id(name), used_ids);
    if service_id != name {
        item_warnings.insert(0, format!("名称 {name} 合法化为服务 id {service_id}"));
    }
    let id_conflict = current_services
        .map(|services| services.contains_key(&service_id))
        .unwrap_or(false);
    if id_conflict {
        item_warnings.push("目标已存在同名服务 id，默认保留现有服务；勾选将覆盖".to_string());
    }
    built.push(BuiltEntry {
        item: ProcfileImportItem {
            name: name.to_string(),
            service_id,
            command: command.to_string(),
            selected: !id_conflict,
            warnings: item_warnings,
            skipped: false,
            id_conflict,
        },
        spec: Some(spec),
    });
}

/// 保守的 shell 语法探测：命中即跳过。宁可少导不错导——quoted 内的元字符也会命中
/// （如 `--query "a|b"`），提示手工配置而不是给出会跑错的草稿。
fn find_shell_metachar(command: &str) -> Option<char> {
    command.chars().find(|&c| {
        matches!(
            c,
            '$' | '`'
                | '|'
                | ';'
                | '>'
                | '<'
                | '&'
                | '('
                | ')'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '~'
        )
    })
}

/// sh 风格引号感知切词：空白分隔；单引号内无转义；双引号内 `\` 转义 `"` `\` `$` 反引号
/// （`$` 已被上层跳过，此处仅为配对容错）。引号不配对 → None。
fn tokenize(command: &str) -> Option<Vec<String>> {
    let mut tokens: Vec<String> = Vec::new();
    let mut cur = String::new();
    let mut has_token = false;
    let mut chars = command.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\'' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(inner) => cur.push(inner),
                        None => return None,
                    }
                }
            }
            '"' => {
                has_token = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(e @ ('"' | '\\' | '`')) => cur.push(e),
                            Some(other) => {
                                cur.push('\\');
                                cur.push(other);
                            }
                            None => return None,
                        },
                        Some(inner) => cur.push(inner),
                        None => return None,
                    }
                }
            }
            '\\' => {
                // 引号外：`\x` 保留 x（转义空白/引号），行尾 `\` 无意义
                match chars.next() {
                    Some(e) => {
                        has_token = true;
                        cur.push(e);
                    }
                    None => {}
                }
            }
            c if c.is_whitespace() => {
                if has_token {
                    tokens.push(std::mem::take(&mut cur));
                    has_token = false;
                }
            }
            c => {
                has_token = true;
                cur.push(c);
            }
        }
    }
    if has_token {
        tokens.push(cur);
    }
    if tokens.is_empty() {
        None
    } else {
        Some(tokens)
    }
}

/// 按 id 规则（`^[A-Za-z][A-Za-z0-9_-]{0,63}$`）合法化：
/// 非法字符替换为 `-`；首字符非字母补 `proc-` 前缀；超长截断。
fn legalize_id(raw: &str) -> String {
    let mut s: String = raw
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
        s.insert_str(0, "proc-");
    }
    if s.len() > MAX_ID_CHARS {
        s.truncate(MAX_ID_CHARS);
    }
    if !is_valid_id(&s) {
        s = "proc".to_string();
    }
    s
}

/// 导入内 id 冲突 → `-proc` 后缀（仍冲突则追加序号）。
fn unique_id(candidate: String, used: &mut BTreeSet<String>) -> String {
    let mut id = candidate.clone();
    if used.contains(&id) {
        id = format!("{candidate}-proc");
        let mut n = 2;
        while used.contains(&id) {
            id = format!("{candidate}-proc-{n}");
            n += 1;
        }
    }
    used.insert(id.clone());
    id
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn temp_ws(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "st-procfile-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_procfile(tag: &str, text: &str) -> PathBuf {
        let dir = temp_ws(tag);
        fs::write(dir.join("Procfile"), text).unwrap();
        dir
    }

    fn preview_items(dir: &Path) -> Vec<ProcfileImportItem> {
        preview(dir, None).unwrap().items
    }

    fn demo_spec() -> SuperTaskFile {
        serde_yaml::from_str(
            "version: 1\nroot: .\nservices:\n  web:\n    kind: node\n    script: dev\n",
        )
        .unwrap()
    }

    #[test]
    fn missing_file_maps_to_procfile_not_found() {
        let dir = temp_ws("missing");
        let err = preview(&dir, None).unwrap_err();
        assert_eq!(err.code(), ErrorCode::ProcfileNotFound);
    }

    #[test]
    fn basic_mapping_program_and_args() {
        let dir = write_procfile("basic", "web: python -m app --port 8080\nworker: sidekiq\n");
        let items = preview_items(&dir);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.selected));

        let merged = apply(&demo_spec(), &dir, &["web".to_string()]).unwrap().0;
        let svc = merged.services.get("web").unwrap();
        assert_eq!(svc.kind, "generic");
        assert_eq!(svc.program.as_deref(), Some("python"));
        assert_eq!(svc.args, vec!["-m", "app", "--port", "8080"]);
        assert_eq!(
            svc.labels.get("origin").map(String::as_str),
            Some("imported")
        );
        assert_eq!(
            svc.labels.get("imported-from").map(String::as_str),
            Some("Procfile:web")
        );
    }

    #[test]
    fn quotes_and_blanks_and_comments() {
        let dir = write_procfile(
            "quotes",
            "# 注释\n\nweb: python 'my app/main.py' \"--title=hello world\"\n   \n",
        );
        let items = preview_items(&dir);
        assert_eq!(items.len(), 1);
        let merged = apply(&demo_spec(), &dir, &["web".to_string()]).unwrap().0;
        let svc = merged.services.get("web").unwrap();
        assert_eq!(svc.program.as_deref(), Some("python"));
        assert_eq!(svc.args, vec!["my app/main.py", "--title=hello world"]);
    }

    #[test]
    fn shell_syntax_is_skipped_not_imported() {
        let dir = write_procfile(
            "shell",
            "web: npm start\npipe: a | b\nsub: a && b\nvar: python serve.py -p $PORT\nredir: a > log.txt\nglob: cat *.txt\ncmd: a;b\n",
        );
        let items = preview_items(&dir);
        let by_id = |id: &str| items.iter().find(|i| i.service_id == id).unwrap();
        assert!(by_id("web").selected);
        for id in ["pipe", "sub", "var", "redir", "glob", "cmd"] {
            let item = by_id(id);
            assert!(item.skipped && !item.selected, "{id} 应跳过");
            assert!(
                item.warnings.iter().any(|w| w.contains("shell 语法")),
                "{id}"
            );
        }
        // apply 显式选择 skipped 项 → 跳过 + 警告，不写入
        let (merged, warnings) = apply(&demo_spec(), &dir, &["var".to_string()]).unwrap();
        assert!(merged.services.get("var").is_none());
        assert!(warnings.iter().any(|w| w.contains("不可导入")));
    }

    #[test]
    fn id_legalization_and_conflict_suffix() {
        let dir = write_procfile(
            "legalize",
            "web.app: node server.js\nweb-app: node server.js\n1abc: node server.js\n",
        );
        let items = preview_items(&dir);
        assert_eq!(items[0].service_id, "web-app");
        assert_eq!(items[1].service_id, "web-app-proc");
        assert_eq!(items[2].service_id, "proc-1abc");
    }

    #[test]
    fn id_conflict_with_existing_service_defaults_keep() {
        let dir = write_procfile("conflict", "web: python serve.py\n");
        let mut current = demo_spec();
        current.services.insert(
            "web".into(),
            ServiceSpec {
                kind: "node".into(),
                script: Some("dev".into()),
                ..ServiceSpec::default_service()
            },
        );
        let out = preview(&dir, Some(&current.services)).unwrap();
        let item = &out.items[0];
        assert!(item.id_conflict && !item.selected);
        // 默认不选 → 旧服务保留
        let (merged, _) = apply(&current, &dir, &[]).unwrap();
        assert_eq!(merged.services.get("web").unwrap().kind, "node");
        // 显式选择 → 覆盖
        let (merged, warnings) = apply(&current, &dir, &["web".to_string()]).unwrap();
        assert_eq!(merged.services.get("web").unwrap().kind, "generic");
        assert!(warnings.iter().any(|w| w.contains("覆盖")));
    }

    #[test]
    fn dotenv_exists_wires_env_file_reference_not_inline() {
        let dir = temp_ws("dotenv");
        fs::write(dir.join("Procfile"), "web: python serve.py\n").unwrap();
        fs::write(dir.join(".env"), "SECRET_TOKEN=super-secret-value\n").unwrap();
        let out = preview(&dir, None).unwrap();
        assert!(out.warnings.iter().any(|w| w.contains(".env")));
        let merged = apply(&demo_spec(), &dir, &["web".to_string()]).unwrap().0;
        let svc = merged.services.get("web").unwrap();
        assert_eq!(svc.env_file, vec![".env".to_string()]);
        // 敏感值不进 yaml（既不在 env 也不在 labels/args）
        let yaml_text = crate::spec::to_yaml(&merged).unwrap();
        assert!(!yaml_text.contains("super-secret-value"), "{yaml_text}");
        assert!(svc.env.is_empty());
    }

    #[test]
    fn duplicate_names_keep_first() {
        let dir = write_procfile("dup", "web: node a.js\nweb: node b.js\n");
        let out = preview(&dir, None).unwrap();
        assert_eq!(out.items.len(), 1);
        assert_eq!(out.items[0].command, "node a.js");
        assert!(out.warnings.iter().any(|w| w.contains("重复")));
    }

    #[test]
    fn malformed_lines_warn_and_continue() {
        let dir = write_procfile(
            "malformed",
            "no-colon-line\nweb: node a.js\n:empty-name\nempty:   \n",
        );
        let out = preview(&dir, None).unwrap();
        assert_eq!(out.items.len(), 1);
        assert!(out.warnings.iter().any(|w| w.contains("name: command")));
    }

    #[test]
    fn apply_rejects_unknown_selected_id() {
        let dir = write_procfile("unknown", "web: node a.js\n");
        let err = apply(&demo_spec(), &dir, &["nope".to_string()]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
    }

    #[test]
    fn empty_procfile_gives_empty_preview_with_warning() {
        let dir = write_procfile("empty", "# 只有注释\n\n");
        let out = preview(&dir, None).unwrap();
        assert!(out.items.is_empty());
        assert!(out.warnings.iter().any(|w| w.contains("没有可识别条目")));
    }

    #[test]
    fn unmatched_quote_is_skipped() {
        let dir = write_procfile("quote", "web: echo \"unterminated\n");
        let items = preview_items(&dir);
        assert!(items[0].skipped);
        assert!(items[0].warnings.iter().any(|w| w.contains("引号不配对")));
    }

    #[test]
    fn preview_is_deterministic() {
        let dir = write_procfile("deterministic", "web: node a.js\nworker: python w.py\n");
        let a = preview(&dir, None).unwrap();
        let b = preview(&dir, None).unwrap();
        assert_eq!(
            serde_json::to_string(&a).unwrap(),
            serde_json::to_string(&b).unwrap()
        );
    }
}
