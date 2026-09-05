//! 1.1 扫描结果 merge：`preview` 生成可重复的增量预览，`apply` 按用户选择合并。
//! 契约：`docs/spec/ipc.md` §10.4、`docs/archive/plans/2026-08-27-v1-1-feature-spec.md` §6。

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};
use crate::spec::{ScriptSpec, ServiceSpec, SuperTaskFile};

/// 扫描器负责的字段。`update` 只覆盖这些字段，其余一律保留 current。
/// 1.4 §5.4：字段所有权扩展 `build_tool`（gradle 草稿带 `build_tool: gradle`）。
const SCANNER_OWNED_FIELDS: &[&str] = &["kind", "module", "dir", "package_manager", "build_tool"];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MergeStatus {
    Added,
    MatchSame,
    MatchDiff,
    Missing,
    IdConflict,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanMergeItem {
    /// 候选键（发现侧 id 或冲突时的发现 id）
    pub service_id: String,
    pub status: MergeStatus,
    pub discovered: Option<ServiceSpec>,
    pub current: Option<ServiceSpec>,
    /// 有差异的扫描器负责字段名
    pub field_diffs: Vec<String>,
    /// IdConflict 时的稳定候选 id
    pub candidate_id: Option<String>,
    /// 默认动作：Added=false，其余=true（保留语义）
    pub selected: bool,
    /// 2.1 README 导入：字段来源（scan/readme/default + 置信度）；普通 scan 预览为 None
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields_meta: Option<Vec<FieldMeta>>,
}

/// 2.1：字段来源元数据（spec §3.4 provenance）。source ∈ scan | readme | default；
/// `readme_value` 仅在「scan 值保留 + README 建议值」冲突时出现（双值展示）。
#[derive(Debug, Clone, Serialize)]
pub struct FieldMeta {
    pub field: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub readme_value: Option<String>,
}

/// id → 字段来源列表（服务用 service_id，脚本用 script_id 作键）。
pub type FieldMetas = indexmap::IndexMap<String, Vec<FieldMeta>>;

/// 2.1：脚本合并项（README 导入的 scripts 草稿走同一向导确认）。
#[derive(Debug, Clone, Serialize)]
pub struct ScriptMergeItem {
    pub script_id: String,
    pub status: MergeStatus,
    pub discovered: Option<ScriptSpec>,
    pub current: Option<ScriptSpec>,
    pub selected: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fields_meta: Option<Vec<FieldMeta>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ScanPreview {
    pub items: Vec<ScanMergeItem>,
    pub warnings: Vec<String>,
    /// 2.1：脚本合并项；普通 scan 预览为空（序列化时省略，老前端不受影响）
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub script_items: Vec<ScriptMergeItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeAction {
    Add,
    Keep,
    Update,
}

#[derive(Debug, Clone, Deserialize)]
pub struct MergeChoice {
    pub id: String,
    pub action: MergeAction,
    #[serde(default)]
    pub fields: Option<Vec<String>>,
    /// 2.1：脚本项为 `script`；缺省 = service（1.1 契约兼容）。
    #[serde(default)]
    pub target: Option<MergeTarget>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MergeTarget {
    Service,
    Script,
}

/// 生成扫描合并预览。匹配规则可重复：相同输入两次调用结果完全一致。
pub fn preview(
    current: &SuperTaskFile,
    discovered: &SuperTaskFile,
    scan_warnings: Vec<String>,
) -> ScanPreview {
    preview_with_sources(current, discovered, scan_warnings, None)
}

/// 2.1：带来源元数据的预览（README 导入）。`sources` 提供 readme 置信度标注；
/// 同时产出 scripts 合并项（普通 scan 预览传 None，script_items 为空）。
pub fn preview_with_sources(
    current: &SuperTaskFile,
    discovered: &SuperTaskFile,
    scan_warnings: Vec<String>,
    sources: Option<(&FieldMetas, &FieldMetas)>,
) -> ScanPreview {
    let items = build_items(current, discovered);
    let items = match sources {
        None => items,
        Some((svc, _)) => items
            .into_iter()
            .map(|mut item| {
                item.fields_meta = svc.get(&item.service_id).cloned();
                item
            })
            .collect(),
    };
    let script_items = build_script_items(current, discovered).map_or_else(Vec::new, |mut v| {
        if let Some((_, scripts)) = sources {
            for item in &mut v {
                item.fields_meta = scripts.get(&item.script_id).cloned();
            }
        }
        v
    });
    ScanPreview {
        items,
        warnings: scan_warnings,
        script_items,
    }
}

/// scripts 合并项：discovered 有而 current 无 → Added；同 id 比较 cmds；
/// current 独有 → Missing。无 IdConflict 概念（脚本无身份字段）。
fn build_script_items(
    current: &SuperTaskFile,
    discovered: &SuperTaskFile,
) -> Option<Vec<ScriptMergeItem>> {
    if discovered.scripts.is_empty() {
        return None;
    }
    let mut items = Vec::new();
    for (id, spec_d) in &discovered.scripts {
        let status = match current.scripts.get(id) {
            None => MergeStatus::Added,
            Some(cur) => {
                if cur.cmds == spec_d.cmds {
                    MergeStatus::MatchSame
                } else {
                    MergeStatus::MatchDiff
                }
            }
        };
        items.push(ScriptMergeItem {
            script_id: id.clone(),
            status,
            discovered: Some(spec_d.clone()),
            current: current.scripts.get(id).cloned(),
            selected: status != MergeStatus::Added,
            fields_meta: None,
        });
    }
    for (id, spec_c) in &current.scripts {
        if !discovered.scripts.contains_key(id) {
            items.push(ScriptMergeItem {
                script_id: id.clone(),
                status: MergeStatus::Missing,
                discovered: None,
                current: Some(spec_c.clone()),
                selected: true,
                fields_meta: None,
            });
        }
    }
    Some(items)
}

/// 按用户选择合并。从 current 克隆出发，reserved 段（templates/git/x-* 等）永不丢。
pub fn apply(
    current: &SuperTaskFile,
    discovered: &SuperTaskFile,
    choices: &[MergeChoice],
) -> Result<SuperTaskFile> {
    let mut out = current.clone();
    if choices.is_empty() {
        return Ok(out);
    }
    let items = build_items(current, discovered);
    let script_items = build_script_items(current, discovered).unwrap_or_default();
    for choice in choices {
        match choice.target {
            Some(MergeTarget::Script) => apply_script_choice(&mut out, &script_items, choice)?,
            _ => apply_service_choice(&mut out, &items, choice)?,
        }
    }
    Ok(out)
}

/// 2.1：脚本项应用。`update` 整体替换（cmds 只来自文档、用户已在向导确认）。
fn apply_script_choice(
    out: &mut SuperTaskFile,
    script_items: &[ScriptMergeItem],
    choice: &MergeChoice,
) -> Result<()> {
    let item = script_items
        .iter()
        .find(|i| i.script_id == choice.id)
        .ok_or_else(|| {
            Error::new(
                ErrorCode::NotFound,
                format!("脚本预览项不存在: {}", choice.id),
            )
        })?;
    match choice.action {
        MergeAction::Keep => {}
        MergeAction::Add => {
            if !matches!(item.status, MergeStatus::Added) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("脚本 {} 不是新增项，无法 add", choice.id),
                ));
            }
            let spec = item
                .discovered
                .as_ref()
                .expect("added 脚本项必带 discovered");
            if out.scripts.contains_key(&choice.id) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("脚本 id 已存在: {}", choice.id),
                ));
            }
            out.scripts.insert(choice.id.clone(), spec.clone());
        }
        MergeAction::Update => {
            let disc = match (item.discovered.as_ref(), item.current.as_ref()) {
                (Some(d), Some(_)) => d,
                _ => {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        format!("脚本 {} 没有可更新的匹配项", choice.id),
                    ))
                }
            };
            out.scripts.insert(choice.id.clone(), disc.clone());
        }
    }
    Ok(())
}

fn apply_service_choice(
    out: &mut SuperTaskFile,
    items: &[ScanMergeItem],
    choice: &MergeChoice,
) -> Result<()> {
    let item = items
        .iter()
        .find(|i| i.service_id == choice.id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("预览项不存在: {}", choice.id)))?;
    match choice.action {
        MergeAction::Keep => {}
        MergeAction::Add => {
            if !matches!(item.status, MergeStatus::Added | MergeStatus::IdConflict) {
                return Err(Error::new(
                    ErrorCode::NotFound,
                    format!("{} 不是新增/冲突项，无法 add", choice.id),
                ));
            }
            let spec = item
                .discovered
                .as_ref()
                .expect("added/conflict 项必带 discovered");
            let key = item
                .candidate_id
                .clone()
                .unwrap_or_else(|| item.service_id.clone());
            if out.services.contains_key(&key) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("id 已存在: {key}"),
                ));
            }
            out.services.insert(key, spec.clone());
        }
        MergeAction::Update => {
            let disc = match (item.discovered.as_ref(), item.current.as_ref()) {
                (Some(d), Some(_)) => d,
                _ => {
                    return Err(Error::new(
                        ErrorCode::NotFound,
                        format!("{} 没有可更新的匹配项", choice.id),
                    ))
                }
            };
            let fields: Vec<&str> = match &choice.fields {
                Some(wanted) => SCANNER_OWNED_FIELDS
                    .iter()
                    .copied()
                    .filter(|f| wanted.iter().any(|w| w == f))
                    .collect(),
                None => SCANNER_OWNED_FIELDS.to_vec(),
            };
            let cur = out
                .services
                .get_mut(&item.service_id)
                .expect("match/conflict 项的 current 必在表内");
            for field in fields {
                match field {
                    "kind" => cur.kind = disc.kind.clone(),
                    "module" => cur.module = disc.module.clone(),
                    "dir" => cur.dir = disc.dir.clone(),
                    "package_manager" => cur.package_manager = disc.package_manager,
                    "build_tool" => cur.build_tool = disc.build_tool.clone(),
                    _ => unreachable!("SCANNER_OWNED_FIELDS 白名单"),
                }
            }
        }
    }
    Ok(())
}

/// 身份一致：kind 相同，且 spring-boot 的 module / node 的 dir / compose 的
/// service（1.3 §7 ②'）相同。
fn same_identity(a: &ServiceSpec, b: &ServiceSpec) -> bool {
    if a.kind != b.kind {
        return false;
    }
    match a.kind.as_str() {
        "spring-boot" => a.module == b.module,
        "node" => a.dir == b.dir,
        // compose 服务身份是 compose 文件内的服务名
        "compose" => a.service.is_some() && a.service == b.service,
        _ => true,
    }
}

/// 只比较扫描器负责字段（None 视为与缺失相同）。
fn field_diffs(discovered: &ServiceSpec, current: &ServiceSpec) -> Vec<String> {
    let mut diffs = Vec::new();
    if discovered.kind != current.kind {
        diffs.push("kind".into());
    }
    if discovered.module != current.module {
        diffs.push("module".into());
    }
    if discovered.dir != current.dir {
        diffs.push("dir".into());
    }
    if discovered.package_manager != current.package_manager {
        diffs.push("package_manager".into());
    }
    if discovered.build_tool != current.build_tool {
        diffs.push("build_tool".into());
    }
    diffs
}

fn push_match_item(
    items: &mut Vec<ScanMergeItem>,
    service_id: &str,
    discovered: &ServiceSpec,
    current: &ServiceSpec,
) {
    let diffs = field_diffs(discovered, current);
    items.push(ScanMergeItem {
        service_id: service_id.into(),
        status: if diffs.is_empty() {
            MergeStatus::MatchSame
        } else {
            MergeStatus::MatchDiff
        },
        discovered: Some(discovered.clone()),
        current: Some(current.clone()),
        field_diffs: diffs,
        candidate_id: None,
        selected: true,
        fields_meta: None,
    });
}

fn is_claimed(claimed: &[&str], id: &str) -> bool {
    claimed.iter().any(|c| *c == id)
}

/// IdConflict 的稳定候选 id：`<id>-2` 起步，避开现有表、发现侧与其他已分配候选。
fn next_candidate(
    base: &str,
    current: &SuperTaskFile,
    discovered: &SuperTaskFile,
    used: &[String],
) -> String {
    for i in 2..99 {
        let cand = format!("{base}-{i}");
        if !current.services.contains_key(&cand)
            && !discovered.services.contains_key(&cand)
            && !used.iter().any(|u| u == &cand)
        {
            return cand;
        }
    }
    format!("{base}-x")
}

/// 匹配（顺序固定，保证可重复）：
/// ① 同 id 且身份一致 → MatchSame/MatchDiff；② 同 id 身份不一致 → IdConflict（占用现有项）；
/// ③ id 不存在但身份与某未占用现有项一致 → 以现有 id 为键 MatchSame/MatchDiff；
/// ④ 其余发现 → Added；⑤ 未被任何发现项对应的现有服务 → Missing。
fn build_items(current: &SuperTaskFile, discovered: &SuperTaskFile) -> Vec<ScanMergeItem> {
    let mut items = Vec::new();
    let mut claimed: Vec<&str> = Vec::new();
    let mut candidates: Vec<String> = Vec::new();

    for (id_d, spec_d) in &discovered.services {
        if let Some(cur) = current.services.get(id_d) {
            if same_identity(cur, spec_d) {
                claimed.push(id_d.as_str());
                push_match_item(&mut items, id_d, spec_d, cur);
                continue;
            }
            claimed.push(id_d.as_str());
            let candidate = next_candidate(id_d, current, discovered, &candidates);
            candidates.push(candidate.clone());
            items.push(ScanMergeItem {
                service_id: id_d.clone(),
                status: MergeStatus::IdConflict,
                discovered: Some(spec_d.clone()),
                current: Some(cur.clone()),
                field_diffs: Vec::new(),
                candidate_id: Some(candidate),
                selected: true,
                fields_meta: None,
            });
            continue;
        }
        let matched = current
            .services
            .iter()
            .find(|(id_c, spec_c)| !is_claimed(&claimed, id_c) && same_identity(spec_c, spec_d));
        if let Some((id_c, spec_c)) = matched {
            claimed.push(id_c.as_str());
            push_match_item(&mut items, id_c, spec_d, spec_c);
            continue;
        }
        items.push(ScanMergeItem {
            service_id: id_d.clone(),
            status: MergeStatus::Added,
            discovered: Some(spec_d.clone()),
            current: None,
            field_diffs: Vec::new(),
            candidate_id: None,
            selected: false,
            fields_meta: None,
        });
    }

    for (id_c, spec_c) in &current.services {
        if !is_claimed(&claimed, id_c) {
            items.push(ScanMergeItem {
                service_id: id_c.clone(),
                status: MergeStatus::Missing,
                discovered: None,
                current: Some(spec_c.clone()),
                field_diffs: Vec::new(),
                candidate_id: None,
                selected: true,
                fields_meta: None,
            });
        }
    }
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use indexmap::IndexMap;

    fn parse(text: &str) -> SuperTaskFile {
        crate::spec::parse_yaml(text).unwrap().0
    }

    fn yaml(file: &SuperTaskFile) -> String {
        crate::spec::to_yaml(file).unwrap()
    }

    fn item<'a>(preview: &'a ScanPreview, id: &str) -> &'a ScanMergeItem {
        preview
            .items
            .iter()
            .find(|i| i.service_id == id)
            .unwrap_or_else(|| panic!("预览缺项 {id}"))
    }

    #[test]
    fn added_preview_and_apply_add() {
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 5173\n",
        );
        let p = preview(&current, &discovered, vec![]);
        let web = item(&p, "web");
        assert_eq!(web.status, MergeStatus::Added);
        assert!(!web.selected);
        assert!(web.current.is_none());
        assert_eq!(item(&p, "api").status, MergeStatus::MatchSame);

        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "web".into(),
                action: MergeAction::Add,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        assert_eq!(out.services.len(), 2);
        let web = out.services.get("web").unwrap();
        assert_eq!(web.kind, "node");
        assert_eq!(web.dir.as_deref(), Some("web"));
    }

    #[test]
    fn identity_match_uses_existing_id() {
        // 发现 id 变了但 kind+module 相同 → 视为同一服务，键用现有 id
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  user-api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n",
        );
        let p = preview(&current, &discovered, vec![]);
        assert_eq!(p.items.len(), 1);
        let m = item(&p, "api");
        assert_eq!(m.status, MergeStatus::MatchSame);
        assert_eq!(m.service_id, "api");

        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "api".into(),
                action: MergeAction::Keep,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        assert!(out.services.contains_key("api"));
        assert!(!out.services.contains_key("user-api"));
    }

    #[test]
    fn match_same_keep_is_noop() {
        // port 是用户字段：不一致也不影响 MatchSame
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 9090\n    env:\n      FOO: \"1\"\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n",
        );
        let p = preview(&current, &discovered, vec![]);
        let m = item(&p, "api");
        assert_eq!(m.status, MergeStatus::MatchSame);
        assert!(m.field_diffs.is_empty());
        assert!(m.selected);

        let kept = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "api".into(),
                action: MergeAction::Keep,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        assert_eq!(yaml(&kept), yaml(&current));

        // 空 choices → 返回 current 克隆
        let empty = apply(&current, &discovered, &[]).unwrap();
        assert_eq!(yaml(&empty), yaml(&current));
    }

    #[test]
    fn match_diff_update_only_scanner_fields() {
        // package_manager 是扫描器负责字段但不参与身份 → 同 id 同 dir 可 MatchDiff
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 5173\n    package_manager: pnpm\n    env:\n      DB_URL: \"jdbc:x\"\n    depends_on: [api]\n    grace_secs: 90\n    extra_args: [\"--port=5173\"]\n    health:\n      type: tcp\n    x-note: 用户备注\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  web:\n    kind: node\n    dir: web\n    port: 5173\n    package_manager: npm\n    x-scan: auto\n",
        );
        let p = preview(&current, &discovered, vec![]);
        assert_eq!(p.items.len(), 2); // web MatchDiff + api Missing
        let m = item(&p, "web");
        assert_eq!(m.status, MergeStatus::MatchDiff);
        assert_eq!(m.field_diffs, vec!["package_manager".to_string()]);

        // update（不限字段）：只覆盖 scanner 字段，其余全部保留
        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "web".into(),
                action: MergeAction::Update,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        let web = out.services.get("web").unwrap();
        assert_eq!(web.package_manager, Some(crate::spec::PackageManager::Npm));
        assert_eq!(web.dir.as_deref(), Some("web"));
        assert_eq!(web.port, Some(5173));
        assert_eq!(web.env.get("DB_URL").map(String::as_str), Some("jdbc:x"));
        assert_eq!(web.depends_on, vec!["api"]);
        assert_eq!(web.grace_secs, Some(90));
        assert_eq!(web.extra_args, vec!["--port=5173".to_string()]);
        assert!(web.health.is_some());
        assert!(web.extra.get("x-note").is_some());
        // discovered 的服务级 x-* 不串入
        assert!(web.extra.get("x-scan").is_none());

        // fields 过滤：交集之外不覆盖
        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "web".into(),
                action: MergeAction::Update,
                fields: Some(vec!["port".into(), "dir".into()]),
                target: None,
            }],
        )
        .unwrap();
        assert_eq!(yaml(&out), yaml(&current));

        // fields 过滤：命中交集时只覆盖该字段
        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "web".into(),
                action: MergeAction::Update,
                fields: Some(vec!["kind".into(), "package_manager".into()]),
                target: None,
            }],
        )
        .unwrap();
        let web = out.services.get("web").unwrap();
        assert_eq!(web.package_manager, Some(crate::spec::PackageManager::Npm));
        assert_eq!(web.env.get("DB_URL").map(String::as_str), Some("jdbc:x"));
        assert_eq!(out.services.get("api").unwrap().port, Some(8080));
    }

    #[test]
    fn missing_kept_and_empty_discovered_all_missing() {
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  legacy:\n    kind: spring-boot\n    module: old-mod\n    port: 8081\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n",
        );
        let p = preview(&current, &discovered, vec![]);
        let m = item(&p, "legacy");
        assert_eq!(m.status, MergeStatus::Missing);
        assert!(m.discovered.is_none());
        assert!(m.current.is_some());
        assert!(m.selected);

        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "legacy".into(),
                action: MergeAction::Keep,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        assert!(out.services.contains_key("legacy"));
        assert!(out.services.contains_key("api"));

        // 空 discovered → 全部 Missing
        let empty = SuperTaskFile {
            version: 1,
            kind: None,
            name: None,
            description: None,
            root: ".".into(),
            env: IndexMap::new(),
            services: IndexMap::new(),
            scripts: IndexMap::new(),
            logging: None,
            secrets: None,
            profiles: None,
            toolchain: None,
            templates: None,
            git: None,
            docker: None,
            gateway: None,
            cloud: None,
            ai: None,
            network: None,
            log_retention: None,
            extra: IndexMap::new(),
        };
        let p = preview(&current, &empty, vec![]);
        assert_eq!(p.items.len(), 2);
        assert!(p
            .items
            .iter()
            .all(|i| i.status == MergeStatus::Missing && i.selected));
    }

    #[test]
    fn id_conflict_stable_candidate_and_apply_add() {
        let current = parse(
            "version: 1\nservices:\n  app:\n    kind: spring-boot\n    module: mod-a\n    port: 8080\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  app:\n    kind: spring-boot\n    module: mod-b\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 5173\n",
        );
        let p1 = preview(&current, &discovered, vec![]);
        let p2 = preview(&current, &discovered, vec![]);
        // 可重复：两次 preview 完全一致
        assert_eq!(
            serde_yaml::to_string(&p1).unwrap(),
            serde_yaml::to_string(&p2).unwrap()
        );
        let c = item(&p1, "app");
        assert_eq!(c.status, MergeStatus::IdConflict);
        assert_eq!(c.candidate_id.as_deref(), Some("app-2"));
        assert!(c.selected);

        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "app".into(),
                action: MergeAction::Add,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        // 现有 app 保留，冲突发现项以 candidate_id 入表
        assert_eq!(
            out.services.get("app").unwrap().module.as_deref(),
            Some("mod-a")
        );
        assert_eq!(
            out.services.get("app-2").unwrap().module.as_deref(),
            Some("mod-b")
        );
    }

    #[test]
    fn update_overrides_user_modified_scanner_field() {
        // 用户自己改了 module → 身份不一致 → IdConflict；显式 update 覆盖它，其余保留
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: user-mod\n    port: 9000\n    env:\n      K: v\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: scanner-mod\n    port: 8080\n",
        );
        let p = preview(&current, &discovered, vec![]);
        let c = item(&p, "api");
        assert_eq!(c.status, MergeStatus::IdConflict);
        assert_eq!(c.candidate_id.as_deref(), Some("api-2"));

        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "api".into(),
                action: MergeAction::Update,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        let api = out.services.get("api").unwrap();
        assert_eq!(api.module.as_deref(), Some("scanner-mod"));
        assert_eq!(api.port, Some(9000));
        assert_eq!(api.env.get("K").map(String::as_str), Some("v"));
        assert!(!out.services.contains_key("api-2"));
    }

    #[test]
    fn roundtrip_reserved_and_x_fields() {
        let current_text = "version: 1\nkind: workspace\nname: rt\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\ntemplates:\n  source: official/spring-boot\n  version: 3\ngit:\n  remote: https://example.com/x.git\nx-custom:\n  anything: [1, 2, 3]\n";
        let current = parse(current_text);
        let (reparsed, _) = crate::spec::parse_yaml(current_text).unwrap();
        assert_eq!(current.templates, reparsed.templates);
        assert_eq!(current.git, reparsed.git);
        assert_eq!(
            current.extra.get("x-custom"),
            reparsed.extra.get("x-custom")
        );

        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 5173\n",
        );
        let out = apply(
            &current,
            &discovered,
            &[
                MergeChoice {
                    id: "web".into(),
                    action: MergeAction::Add,
                    fields: None,
                    target: None,
                },
                MergeChoice {
                    id: "api".into(),
                    action: MergeAction::Keep,
                    fields: None,
                    target: None,
                },
            ],
        )
        .unwrap();
        // to_yaml 再 parse_yaml，reserved 段原样存在
        let (back, _) = crate::spec::parse_yaml(&yaml(&out)).unwrap();
        assert_eq!(back.templates, current.templates);
        assert_eq!(back.git, current.git);
        assert_eq!(back.extra.get("x-custom"), current.extra.get("x-custom"));
        assert!(back.services.contains_key("web"));
        assert!(back.services.contains_key("api"));
    }

    #[test]
    fn preview_is_deterministic() {
        // 混合场景：匹配 + 冲突 + 新增 + 未发现，多次 preview 结果一致
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  app:\n    kind: spring-boot\n    module: mod-a\n    port: 8081\n  gone:\n    kind: node\n    dir: old-web\n    port: 5173\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  app:\n    kind: spring-boot\n    module: mod-b\n    port: 8080\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 5173\n",
        );
        let a = serde_yaml::to_string(&preview(&current, &discovered, vec!["w".into()])).unwrap();
        let b = serde_yaml::to_string(&preview(&current, &discovered, vec!["w".into()])).unwrap();
        assert_eq!(a, b);
        let p = preview(&current, &discovered, vec![]);
        assert_eq!(item(&p, "app").status, MergeStatus::IdConflict);
        assert_eq!(item(&p, "api").status, MergeStatus::MatchSame);
        assert_eq!(item(&p, "web").status, MergeStatus::Added);
        assert_eq!(item(&p, "gone").status, MergeStatus::Missing);
    }

    #[test]
    fn compose_identity_matches_by_service_name() {
        // 1.3 §7 ②'：kind: compose 且 service 相同 → 视为同一服务（MatchSame）
        let current = parse(
            "version: 1\nservices:\n  redis:\n    kind: compose\n    service: redis\n    port: 6379\ndocker:\n  compose_file: compose.yaml\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  cache:\n    kind: compose\n    service: redis\n    port: 6380\ndocker:\n  compose_file: compose.yaml\n",
        );
        let p = preview(&current, &discovered, vec![]);
        assert_eq!(p.items.len(), 1);
        let m = item(&p, "redis");
        assert_eq!(m.status, MergeStatus::MatchSame);
        assert!(m.field_diffs.is_empty());

        // service 不同 → 不是同一服务（Added + Missing）
        let discovered2 =
            parse("version: 1\nservices:\n  redis:\n    kind: compose\n    service: mysql\n");
        let p2 = preview(&current, &discovered2, vec![]);
        assert_eq!(item(&p2, "redis").status, MergeStatus::IdConflict);

        // add 后 service 字段入库
        let out = apply(
            &current,
            &discovered2,
            &[MergeChoice {
                id: "redis".into(),
                action: MergeAction::Add,
                fields: None,
                target: None,
            }],
        )
        .unwrap();
        assert_eq!(
            out.services.get("redis-2").unwrap().service.as_deref(),
            Some("mysql")
        );
    }

    #[test]
    fn build_tool_field_ownership_14() {
        // 1.4 §5.4：build_tool 是扫描器负责字段
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    build_tool: maven\n    port: 8080\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    build_tool: gradle\n    port: 8081\n",
        );
        let p = preview(&current, &discovered, vec![]);
        let m = item(&p, "api");
        assert_eq!(m.status, MergeStatus::MatchDiff);
        assert_eq!(m.field_diffs, vec!["build_tool".to_string()]);

        // fields 过滤：只选 build_tool → 只覆盖它，用户字段保留
        let out = apply(
            &current,
            &discovered,
            &[MergeChoice {
                id: "api".into(),
                action: MergeAction::Update,
                fields: Some(vec!["build_tool".into()]),
                target: None,
            }],
        )
        .unwrap();
        assert_eq!(out.services["api"].build_tool.as_deref(), Some("gradle"));
        assert_eq!(out.services["api"].port, Some(8080));
    }

    #[test]
    fn apply_errors() {
        let current = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  legacy:\n    kind: spring-boot\n    module: old-mod\n    port: 8081\n",
        );
        let discovered = parse(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: server-api\n    port: 8080\n  web:\n    kind: node\n    dir: web\n    port: 5173\n",
        );
        let choice = |id: &str, action: MergeAction| MergeChoice {
            id: id.into(),
            action,
            fields: None,
            target: None,
        };
        // 未知 id
        let err = apply(&current, &discovered, &[choice("nope", MergeAction::Keep)]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        // add 只对 Added/IdConflict 有效
        let err = apply(&current, &discovered, &[choice("api", MergeAction::Add)]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        let err = apply(&current, &discovered, &[choice("legacy", MergeAction::Add)]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        // update 只对有 discovered+current 的项有效
        let err = apply(&current, &discovered, &[choice("web", MergeAction::Update)]).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        // 重复 add 同一项 → id 已存在
        let err = apply(
            &current,
            &discovered,
            &[
                choice("web", MergeAction::Add),
                choice("web", MergeAction::Add),
            ],
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::SpecInvalid);
    }
}
