//! 模板模块：内置官方模板 + 本地用户模板，统一「目录 + template.yaml 清单」模型。
//!
//! 规格来源：`docs/archive/plans/2026-08-27-v1-1-feature-spec.md` §4 与
//! `docs/archive/plans/2026-08-28-templates-upgrade-plan.md`（模板来源升级）：
//! - builtin 资源用 `include_dir` 编译期嵌入（`crates/supertask-core/template_assets/`），
//!   离线分发，不依赖网络；
//! - local 模板来自用户目录（`%APPDATA%/SuperTask/templates/<id>/`），与 builtin
//!   同一份 `template.yaml` 清单格式、同一条创建管线；
//! - 清单声明元数据与 files 列表，与目录实际文件的双向一致性由校验和单元测试兜底；
//! - `create_template` 先做目录安全与目标检查，复制前校验文件集合，失败不落盘；
//!   写失败不删除已复制文件，也不显示成功。

use std::collections::{BTreeMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};
use crate::sandbox::strip_verbatim;
use crate::spec::{parse_yaml, SuperTaskFile};

/// 编译期嵌入 `crates/supertask-core/template_assets` 下全部模板资源。
static TEMPLATE_ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/template_assets");

/// 嵌入/本地目录里忽略的构建产物、版本库与清单外的隐藏目录。
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist"];

/// 模板清单文件名（模板目录根；不属于模板产物，不复制到目标工作区）。
const MANIFEST_FILE: &str = "template.yaml";

/// 模板来源（IPC 输出序列化为 `"builtin"` / `"local"`）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TemplateSourceKind {
    Builtin,
    Local,
}

impl TemplateSourceKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Builtin => "builtin",
            Self::Local => "local",
        }
    }
}

/// 模板概览（IPC `templates.list` 输出项）。
#[derive(Debug, Clone, Serialize)]
pub struct TemplateSummary {
    pub id: String,
    pub version: String,
    pub name: String,
    pub description: String,
    pub stacks: Vec<String>,
    /// 模板内相对路径概览（`/` 分隔），只用于展示与校验，不由前端解释。
    pub files: Vec<String>,
    pub source: TemplateSourceKind,
    /// 仅 local：清单缺失/损坏时为 true，其余字段不可信，禁止创建。
    pub invalid: bool,
    pub invalid_reason: Option<String>,
    /// 创建参数声明（无参数模板为 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Vec<TemplateParam>>,
    /// 组合模板的服务块声明（非组合模板为 None）。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub blocks: Option<Vec<TemplateBlockSummary>>,
}

/// 创建参数声明（清单 `params` 段）：创建时 `{{key}}` 替换进 UTF-8 文本文件，
/// `apply_to: [yaml.name]` 额外覆写生成工作区 yaml 的 name。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateParam {
    pub key: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub required: bool,
    /// 目前仅支持 `yaml.name`；其他目标在清单解析期拒绝。
    #[serde(default)]
    pub apply_to: Vec<String>,
}

/// 组合模板的服务块声明（清单 `blocks` 段）：块 = 文件 + services 片段。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TemplateBlock {
    pub id: String,
    #[serde(default)]
    pub label: String,
    /// 展示用技术栈标记（spring-boot / node …），由清单声明。
    #[serde(default)]
    pub kind: String,
    /// 依赖的其他块 id；创建/预览时自动闭合。
    #[serde(default)]
    pub requires: Vec<String>,
    /// 该块服务的缺省端口（services 片段未写 port 时或向导未指定时使用）。
    #[serde(default)]
    pub default_port: Option<u32>,
    /// 块包含的模板文件（相对路径）。
    #[serde(default)]
    pub files: Vec<String>,
    /// 注入生成 supertask.yaml `services` 的片段（键 = 服务 id，缺 port 时由端口分配补齐）。
    #[serde(default)]
    pub services: Value,
}

/// 块概览（进 IPC TemplateSummary，供向导渲染）。
#[derive(Debug, Clone, Serialize)]
pub struct TemplateBlockSummary {
    pub id: String,
    pub label: String,
    pub kind: String,
    pub requires: Vec<String>,
    pub default_port: Option<u32>,
    /// 块内声明的服务 id（端口分配的键）。
    pub services: Vec<String>,
}

/// `template.yaml` 清单（builtin 与 local 同格式）。
#[derive(Debug, Clone, Deserialize)]
struct TemplateManifest {
    id: String,
    version: String,
    name: String,
    description: String,
    #[serde(default)]
    stacks: Vec<String>,
    /// 组合模板可省略（files = 块文件并集）；普通模板必填。
    #[serde(default)]
    files: Vec<String>,
    #[serde(default)]
    params: Vec<TemplateParam>,
    #[serde(default)]
    blocks: Vec<TemplateBlock>,
}

/// 一个已定位的模板：概览 + 内容读取方式。
struct TemplateEntry {
    summary: TemplateSummary,
    /// 清单声明的创建参数（校验与替换用）。
    params: Vec<TemplateParam>,
    /// 组合块（空 = 非组合模板）。
    blocks: Vec<TemplateBlock>,
    kind: TemplateKind,
}

enum TemplateKind {
    Builtin(&'static Dir<'static>),
    Local(PathBuf),
}

/// 解析并校验 template.yaml；id 必须与所在目录名一致，files 必须是
/// 安全的相对路径且不得包含清单自身。
fn parse_manifest(dir_name: &str, bytes: &[u8]) -> Result<TemplateManifest> {
    let text = std::str::from_utf8(bytes)
        .map_err(|_| Error::new(ErrorCode::TemplateInvalid, "template.yaml 不是 UTF-8"))?;
    let manifest: TemplateManifest = serde_yaml::from_str(text).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("template.yaml 解析失败: {e}"),
        )
    })?;
    let invalid =
        |why: String| Error::new(ErrorCode::TemplateInvalid, format!("template.yaml: {why}"));
    if manifest.id != dir_name {
        return Err(invalid(format!(
            "id {:?} 与目录名 {dir_name:?} 不一致",
            manifest.id
        )));
    }
    if manifest.version.is_empty() || manifest.name.is_empty() {
        return Err(invalid("version/name 不能为空".into()));
    }
    if manifest.files.is_empty() && manifest.blocks.is_empty() {
        return Err(invalid(
            "files 不能为空（组合模板可用 blocks 携带文件）".into(),
        ));
    }
    if manifest.files.iter().any(|f| f == MANIFEST_FILE) {
        return Err(invalid(format!("files 不得包含 {MANIFEST_FILE}")));
    }
    for f in &manifest.files {
        let bad = f.is_empty()
            || f.starts_with('/')
            || f.starts_with('\\')
            || f.contains(':')
            || f.split(['/']).any(|seg| seg == ".." || seg.is_empty())
            || f.contains('\\');
        if bad {
            return Err(invalid(format!("files 含非法相对路径 {f:?}")));
        }
    }
    validate_params(&manifest.params)?;
    validate_blocks(&manifest.blocks)?;
    Ok(manifest)
}

/// 组合块校验：id 唯一、requires 引用已声明块、files 相对安全且不含
/// supertask.yaml/template.yaml、services 为非空映射、服务 id 跨块唯一。
fn validate_blocks(blocks: &[TemplateBlock]) -> Result<()> {
    let invalid = |id: &str, why: String| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("template.yaml blocks[{id}]: {why}"),
        )
    };
    let mut ids = HashSet::new();
    let mut service_ids = HashSet::new();
    for b in blocks {
        if b.id.is_empty() {
            return Err(invalid("<匿名>", "id 不能为空".into()));
        }
        if !ids.insert(b.id.clone()) {
            return Err(invalid(&b.id, "id 重复".into()));
        }
        for r in &b.requires {
            if !blocks.iter().any(|x| &x.id == r) {
                return Err(invalid(&b.id, format!("requires 引用未声明的块 {r:?}")));
            }
        }
        for f in &b.files {
            let bad = f.is_empty()
                || f == MANIFEST_FILE
                || f == "supertask.yaml"
                || f.starts_with('/')
                || f.starts_with('\\')
                || f.contains(':')
                || f.split(['/']).any(|seg| seg == ".." || seg.is_empty())
                || f.contains('\\');
            if bad {
                return Err(invalid(&b.id, format!("files 含非法路径 {f:?}")));
            }
        }
        let Some(services) = b.services.as_mapping() else {
            return Err(invalid(&b.id, "services 必须是映射".into()));
        };
        if services.is_empty() {
            return Err(invalid(&b.id, "services 不能为空".into()));
        }
        for (k, _) in services {
            let Some(svc_id) = k.as_str() else {
                return Err(invalid(&b.id, "services 键必须是字符串".into()));
            };
            if !service_ids.insert(svc_id.to_string()) {
                return Err(invalid(&b.id, format!("服务 id {svc_id:?} 跨块重复")));
            }
        }
    }
    Ok(())
}

/// 组合模板的文件总集（各块 files 的并集，保持清单顺序、去重）。
fn block_files_union(blocks: &[TemplateBlock]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for b in blocks {
        for f in &b.files {
            if !out.contains(f) {
                out.push(f.clone());
            }
        }
    }
    out
}

fn block_summaries(blocks: &[TemplateBlock]) -> Vec<TemplateBlockSummary> {
    blocks
        .iter()
        .map(|b| TemplateBlockSummary {
            id: b.id.clone(),
            label: if b.label.is_empty() {
                b.id.clone()
            } else {
                b.label.clone()
            },
            kind: b.kind.clone(),
            requires: b.requires.clone(),
            default_port: b.default_port,
            services: b
                .services
                .as_mapping()
                .map(|m| {
                    m.keys()
                        .filter_map(|k| k.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default(),
        })
        .collect()
}

/// 参数声明校验：key 命名约束 + apply_to 目标白名单。
fn validate_params(params: &[TemplateParam]) -> Result<()> {
    let mut seen = HashSet::new();
    for p in params {
        let invalid = |why: String| {
            Error::new(
                ErrorCode::TemplateInvalid,
                format!("template.yaml params[{}]: {why}", p.key),
            )
        };
        if !p.key.is_empty()
            && p.key
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
            && !p.key.chars().next().is_some_and(|c| c.is_ascii_digit())
        {
            // 命名合法
        } else {
            return Err(invalid(
                "key 只允许小写字母/数字/下划线且不以数字开头".into(),
            ));
        }
        if !seen.insert(p.key.clone()) {
            return Err(invalid("key 重复".into()));
        }
        for target in &p.apply_to {
            if target != "yaml.name" {
                return Err(invalid(format!(
                    "apply_to 目标 {target:?} 不存在（仅支持 yaml.name）"
                )));
            }
        }
    }
    Ok(())
}

fn invalid_summary(id: &str, source: TemplateSourceKind, reason: String) -> TemplateSummary {
    TemplateSummary {
        id: id.to_string(),
        version: String::new(),
        name: id.to_string(),
        description: reason.clone(),
        stacks: Vec::new(),
        files: Vec::new(),
        source,
        invalid: true,
        invalid_reason: Some(reason),
        params: None,
        blocks: None,
    }
}

/// 从清单构造模板条目（此时不读文件内容，延迟到 create 校验期）。
fn entry_from_manifest(manifest: TemplateManifest, kind: TemplateKind) -> TemplateEntry {
    let source = match kind {
        TemplateKind::Builtin(_) => TemplateSourceKind::Builtin,
        TemplateKind::Local(_) => TemplateSourceKind::Local,
    };
    let params = manifest.params;
    let blocks = manifest.blocks;
    // 组合模板：概览 files = 块文件并集（清单顶层 files 可省略）
    let files = if manifest.files.is_empty() && !blocks.is_empty() {
        block_files_union(&blocks)
    } else {
        manifest.files
    };
    TemplateEntry {
        summary: TemplateSummary {
            id: manifest.id,
            version: manifest.version,
            name: manifest.name,
            description: manifest.description,
            stacks: manifest.stacks,
            files,
            source,
            invalid: false,
            invalid_reason: None,
            params: if params.is_empty() {
                None
            } else {
                Some(params.clone())
            },
            blocks: if blocks.is_empty() {
                None
            } else {
                Some(block_summaries(&blocks))
            },
        },
        params,
        blocks,
        kind,
    }
}

/// 读取 builtin 模板目录的清单；缺失/损坏返回 None（list 里降级为 invalid 条目）。
fn builtin_entry(dir: &'static Dir<'static>) -> Option<TemplateEntry> {
    let dir_name = dir.path().file_name()?.to_str()?;
    // include_dir 的 get_file 需要相对嵌入根的完整路径（带模板 id 前缀）
    let manifest_path = format!("{dir_name}/{MANIFEST_FILE}");
    let bytes = dir.get_file(manifest_path.as_str())?.contents();
    match parse_manifest(dir_name, bytes) {
        Ok(manifest) => Some(entry_from_manifest(manifest, TemplateKind::Builtin(dir))),
        Err(e) => Some(TemplateEntry {
            summary: invalid_summary(
                dir_name,
                TemplateSourceKind::Builtin,
                e.message().to_string(),
            ),
            params: Vec::new(),
            blocks: Vec::new(),
            kind: TemplateKind::Builtin(dir),
        }),
    }
}

/// 内置模板条目（发现 = 扫描嵌入根下的子目录，加模板不改本文件）。
fn builtin_entries() -> Vec<TemplateEntry> {
    let mut entries: Vec<TemplateEntry> = Vec::new();
    for dir in TEMPLATE_ASSETS.dirs() {
        if let Some(entry) = builtin_entry(dir) {
            entries.push(entry);
        }
    }
    entries.sort_by(|a, b| a.summary.id.cmp(&b.summary.id));
    entries
}

/// 扫描本地模板目录；返回 (条目, 与 builtin 冲突被跳过的 id 列表)。
fn local_entries(
    local_dir: &Path,
    builtin_ids: &HashSet<String>,
) -> (Vec<TemplateEntry>, Vec<String>) {
    let mut entries = Vec::new();
    let mut skipped = Vec::new();
    let Ok(read) = fs::read_dir(local_dir) else {
        return (entries, skipped);
    };
    for item in read.flatten() {
        let path = item.path();
        if !path.is_dir() {
            continue;
        }
        let Some(dir_name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if builtin_ids.contains(dir_name) {
            skipped.push(dir_name.to_string());
            continue;
        }
        let manifest_path = path.join(MANIFEST_FILE);
        let entry = match fs::read(&manifest_path) {
            Ok(bytes) => match parse_manifest(dir_name, &bytes) {
                Ok(manifest) => entry_from_manifest(manifest, TemplateKind::Local(path)),
                Err(e) => TemplateEntry {
                    summary: invalid_summary(
                        dir_name,
                        TemplateSourceKind::Local,
                        e.message().to_string(),
                    ),
                    params: Vec::new(),
                    blocks: Vec::new(),
                    kind: TemplateKind::Local(path),
                },
            },
            Err(_) => TemplateEntry {
                summary: invalid_summary(
                    dir_name,
                    TemplateSourceKind::Local,
                    format!("缺少 {MANIFEST_FILE}"),
                ),
                params: Vec::new(),
                blocks: Vec::new(),
                kind: TemplateKind::Local(path),
            },
        };
        entries.push(entry);
    }
    entries.sort_by(|a, b| a.summary.id.cmp(&b.summary.id));
    (entries, skipped)
}

fn builtin_ids() -> HashSet<String> {
    builtin_entries()
        .into_iter()
        .map(|e| e.summary.id)
        .collect()
}

/// 枚举模板（IPC `templates.list`）：builtin 恒在，`local_dir` 提供本地模板。
/// 与 builtin 同 id 的 local 模板被跳过（builtin 优先）。
pub fn list_templates(local_dir: Option<&Path>) -> Vec<TemplateSummary> {
    let mut out: Vec<TemplateSummary> = builtin_entries().into_iter().map(|e| e.summary).collect();
    if let Some(dir) = local_dir {
        let ids = out.iter().map(|s| s.id.clone()).collect::<HashSet<_>>();
        let (locals, _) = local_entries(dir, &ids);
        out.extend(locals.into_iter().map(|e| e.summary));
    }
    out
}

/// 定位模板：builtin 直接查嵌入资源；local 先拒绝与 builtin 的 id 冲突。
fn resolve_entry(
    template_id: &str,
    source: TemplateSourceKind,
    local_dir: Option<&Path>,
) -> Result<TemplateEntry> {
    match source {
        TemplateSourceKind::Builtin => builtin_entries()
            .into_iter()
            .find(|e| e.summary.id == template_id)
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("模板不存在: {template_id}"))),
        TemplateSourceKind::Local => {
            if builtin_ids().contains(template_id) {
                return Err(Error::new(
                    ErrorCode::TemplateIdConflict,
                    format!("本地模板 id {template_id:?} 与内置模板冲突"),
                ));
            }
            let dir = local_dir
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "本地模板目录不可用".to_string()))?;
            let (entries, _) = local_entries(dir, &builtin_ids());
            entries
                .into_iter()
                .find(|e| e.summary.id == template_id)
                .ok_or_else(|| {
                    Error::new(
                        ErrorCode::NotFound,
                        format!("本地模板不存在: {template_id}"),
                    )
                })
        }
    }
}

/// 用模板在 `parent_path/directory_name` 创建新工作区，返回工作区根目录。
/// `params` 的键值用于 `{{key}}` 替换与 `apply_to` 覆写，未声明/缺失键在落盘前拒绝。
/// 组合模板（有 blocks）按 `blocks` 选择生成（None = 全块）；`ports` 是
/// 服务 id → 端口的向导分配结果，缺省用块 `default_port`。
pub fn create_template(
    template_id: &str,
    source: TemplateSourceKind,
    parent_path: &Path,
    directory_name: &str,
    local_dir: Option<&Path>,
    params: &BTreeMap<String, String>,
    blocks: Option<&[String]>,
    ports: &BTreeMap<String, u32>,
) -> Result<PathBuf> {
    let entry = resolve_entry(template_id, source, local_dir)?;
    if entry.summary.invalid {
        return Err(Error::new(
            ErrorCode::TemplateInvalid,
            entry
                .summary
                .invalid_reason
                .unwrap_or_else(|| "模板清单损坏".into()),
        ));
    }

    validate_param_values(&entry, params)?;
    let plan = plan_blocks(&entry, blocks, ports)?;
    validate_directory_name(directory_name)?;

    let parent = fs::canonicalize(parent_path).map_err(|_| {
        Error::new(
            ErrorCode::CwdMissing,
            format!("父目录不存在: {}", parent_path.display()),
        )
    })?;
    let parent = strip_verbatim(parent);

    // 目标不存在则创建；存在且为空可继续；非空（或被同名文件占用）拒绝且不动原内容
    let target = parent.join(directory_name);
    if target.exists() {
        if !target.is_dir() {
            return Err(Error::new(
                ErrorCode::TargetNotEmpty,
                format!("目标路径已存在且不是目录: {}", target.display()),
            ));
        }
        let has_entries = fs::read_dir(&target)
            .map_err(|e| {
                Error::new(
                    ErrorCode::TemplateWrite,
                    format!("无法读取目标目录 {}: {e}", target.display()),
                )
            })?
            .next()
            .is_some();
        if has_entries {
            return Err(Error::new(
                ErrorCode::TargetNotEmpty,
                format!("目标目录非空: {}", target.display()),
            ));
        }
    } else {
        fs::create_dir_all(&target).map_err(|e| {
            Error::new(
                ErrorCode::TemplateWrite,
                format!("无法创建目标目录 {}: {e}", target.display()),
            )
        })?;
    }

    // 复制前先校验模板文件集合与清单一致，失败不落盘
    let mut files = verify_entry_files(&entry)?;
    apply_params(&mut files, params);

    let yaml_text = if let Some(plan) = &plan {
        // 组合模板：只写选中块的文件（模板不含 supertask.yaml，yaml 由块片段生成）
        let selected: HashSet<&str> = plan.files.iter().map(|s| s.as_str()).collect();
        files.retain(|(rel, _)| selected.contains(rel.as_str()));
        for (rel, bytes) in &files {
            write_file(&target, rel, bytes)?;
        }
        build_blocks_workspace_yaml(&entry, plan, params)?
    } else {
        // 普通模板：逐文件复制；supertask.yaml 注入 templates 保留段后最后写
        for (rel, bytes) in &files {
            if rel == "supertask.yaml" {
                continue;
            }
            write_file(&target, rel, bytes)?;
        }

        let template_yaml = files
            .iter()
            .find(|(rel, _)| rel == "supertask.yaml")
            .map(|(_, bytes)| bytes.as_slice())
            .ok_or_else(|| Error::new(ErrorCode::TemplateInvalid, "模板缺少 supertask.yaml"))?;
        build_workspace_yaml(template_yaml, &entry, params)?
    };
    let yaml_path = target.join("supertask.yaml");
    fs::write(&yaml_path, &yaml_text).map_err(|e| {
        Error::new(
            ErrorCode::TemplateWrite,
            format!("写入失败: {}: {e}", yaml_path.display()),
        )
    })?;

    // 写盘后用 parse_yaml 复核；失败保留已复制文件，提示手动修复
    parse_yaml(&yaml_text).map_err(|e| {
        Error::new(
            ErrorCode::YamlParse,
            format!("模板生成的 supertask.yaml 校验失败: {e}"),
        )
    })?;

    // 规格要求：创建完成后对关键文件做存在性校验（组合模板 = 选中块并集）
    let expect_files = plan
        .as_ref()
        .map(|p| p.files.clone())
        .unwrap_or_else(|| entry.summary.files.clone());
    for rel in &expect_files {
        let dest = target.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        if !dest.is_file() {
            return Err(Error::new(
                ErrorCode::TemplateWrite,
                format!("创建后缺少文件: {rel}"),
            ));
        }
    }

    Ok(target)
}

/// `directory_name` 必须是单层目录名：拒绝空、`.`/`..`、路径分隔符（含 UNC
/// 前缀 `\\` / `//`）、盘符冒号、Windows 非法字符、保留设备名与结尾点/空格。
fn validate_directory_name(name: &str) -> Result<()> {
    let reject =
        |why: String| Error::new(ErrorCode::PathEscape, format!("非法目录名 {name:?}: {why}"));
    if name.is_empty() {
        return Err(reject("不能为空".into()));
    }
    if name == "." || name == ".." {
        return Err(reject("不允许 . 或 ..".into()));
    }
    if name.starts_with(r"\\") || name.starts_with("//") {
        return Err(reject("不允许 UNC 路径".into()));
    }
    if name.contains('/') || name.contains('\\') {
        return Err(reject("不能包含路径分隔符".into()));
    }
    if name.contains(':') {
        return Err(reject("不能包含盘符分隔符 ':'".into()));
    }
    if name
        .chars()
        .any(|c| c.is_control() || matches!(c, '"' | '<' | '>' | '|' | '*' | '?'))
    {
        return Err(reject("包含 Windows 文件名非法字符".into()));
    }
    const RESERVED: &[&str] = &[
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
        "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
    ];
    let base = name.split('.').next().unwrap_or("");
    if RESERVED.contains(&base.to_ascii_uppercase().as_str()) {
        return Err(reject("Windows 保留设备名".into()));
    }
    if name.ends_with('.') || name.ends_with(' ') {
        return Err(reject("不能以点或空格结尾".into()));
    }
    Ok(())
}

/// 校验模板文件集合与清单 files 完全一致（双向），通过后返回
/// (相对路径, 内容) 列表（按路径升序）；清单自身不参与。
fn verify_entry_files(entry: &TemplateEntry) -> Result<Vec<(String, Vec<u8>)>> {
    let mut actual: Vec<(String, Vec<u8>)> = Vec::new();
    match &entry.kind {
        TemplateKind::Builtin(dir) => {
            let mut embedded: Vec<(String, &'static [u8])> = Vec::new();
            collect_embedded_files(dir, &mut embedded);
            // 兼容 include_dir 的路径语义差异：条目路径可能带模板 id 前缀
            let prefix = format!("{}/", entry.summary.id);
            for (path, bytes) in embedded {
                let rel = bytes_strip_prefix(&path, &prefix);
                actual.push((rel, bytes.to_vec()));
            }
        }
        TemplateKind::Local(root) => {
            collect_local_files(root, root, &mut actual)?;
        }
    }
    actual.retain(|(path, _)| path != MANIFEST_FILE);
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    // 组合模板：期望集合 = 所选块的文件并集；普通模板：清单 files
    let mut expected_owned = if entry.blocks.is_empty() {
        entry.summary.files.clone()
    } else {
        block_files_union(&entry.blocks)
    };
    expected_owned.sort();
    let expected: Vec<&str> = expected_owned.iter().map(|s| s.as_str()).collect();
    let actual_paths: Vec<&str> = actual.iter().map(|(p, _)| p.as_str()).collect();
    if actual_paths != expected {
        let missing: Vec<&str> = expected
            .iter()
            .filter(|e| !actual_paths.contains(e))
            .copied()
            .collect();
        let extra: Vec<&str> = actual_paths
            .iter()
            .filter(|a| !expected.contains(a))
            .copied()
            .collect();
        return Err(Error::new(
            ErrorCode::TemplateInvalid,
            format!(
                "模板 {} 文件清单不一致：缺少 {missing:?}，多余 {extra:?}",
                entry.summary.id
            ),
        ));
    }
    Ok(actual)
}

fn bytes_strip_prefix(path: &str, prefix: &str) -> String {
    path.strip_prefix(prefix).unwrap_or(path).to_string()
}

/// 递归收集嵌入目录内的文件（相对模板根、`/` 分隔），跳过构建产物目录。
fn collect_embedded_files(dir: &'static Dir<'static>, out: &mut Vec<(String, &'static [u8])>) {
    for file in dir.files() {
        out.push((
            file.path().to_string_lossy().replace('\\', "/"),
            file.contents(),
        ));
    }
    for sub in dir.dirs() {
        if let Some(name) = sub.path().file_name().and_then(|n| n.to_str()) {
            if SKIP_DIRS.contains(&name) || name.starts_with('.') {
                continue;
            }
        }
        collect_embedded_files(sub, out);
    }
}

/// 递归收集本地模板目录内的文件（相对模板根、`/` 分隔）。
fn collect_local_files(root: &Path, dir: &Path, out: &mut Vec<(String, Vec<u8>)>) -> Result<()> {
    let read = fs::read_dir(dir).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("无法读取模板目录 {}: {e}", dir.display()),
        )
    })?;
    for item in read.flatten() {
        let path = item.path();
        let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
            continue;
        };
        if path.is_dir() {
            if SKIP_DIRS.contains(&name) || name.starts_with('.') {
                continue;
            }
            collect_local_files(root, &path, out)?;
        } else if name.starts_with('.') {
            continue;
        } else {
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            let bytes = fs::read(&path).map_err(|e| {
                Error::new(
                    ErrorCode::TemplateInvalid,
                    format!("无法读取模板文件 {}: {e}", path.display()),
                )
            })?;
            out.push((rel, bytes));
        }
    }
    Ok(())
}

/// 复制单个模板文件到目标目录；失败返回 `TEMPLATE_WRITE`，message 含失败路径。
fn write_file(target: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
    let dest = target.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Error::new(
                ErrorCode::TemplateWrite,
                format!("无法创建目录 {}: {e}", parent.display()),
            )
        })?;
    }
    fs::write(&dest, bytes).map_err(|e| {
        Error::new(
            ErrorCode::TemplateWrite,
            format!("写入失败: {}: {e}", dest.display()),
        )
    })
}

/// 取模板自带 supertask.yaml，注入 `templates` 保留段后重新序列化；
/// `apply_to: [yaml.name]` 的参数在此覆写 name。
fn build_workspace_yaml(
    template_yaml: &[u8],
    entry: &TemplateEntry,
    params: &BTreeMap<String, String>,
) -> Result<String> {
    let text = std::str::from_utf8(template_yaml)
        .map_err(|_| Error::new(ErrorCode::TemplateInvalid, "模板 supertask.yaml 不是 UTF-8"))?;
    let mut file: SuperTaskFile = serde_yaml::from_str(text).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板 supertask.yaml 解析失败: {e}"),
        )
    })?;
    for p in &entry.params {
        if p.apply_to.iter().any(|t| t == "yaml.name") {
            if let Some(v) = params.get(&p.key) {
                file.name = Some(v.clone());
            }
        }
    }
    let mut section = serde_yaml::Mapping::new();
    section.insert(
        Value::from("source"),
        Value::from(entry.summary.source.as_str()),
    );
    section.insert(Value::from("id"), Value::from(entry.summary.id.as_str()));
    section.insert(
        Value::from("version"),
        Value::from(entry.summary.version.as_str()),
    );
    file.templates = Some(Value::Mapping(section));
    serde_yaml::to_string(&file).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板 supertask.yaml 序列化失败: {e}"),
        )
    })
}

/// 校验调用方传入的参数：required 缺失 → TEMPLATE_PARAM_MISSING；
/// 未声明的键 → TEMPLATE_PARAM_UNKNOWN；值不允许空白。
fn validate_param_values(entry: &TemplateEntry, params: &BTreeMap<String, String>) -> Result<()> {
    for (k, v) in params {
        if !entry.params.iter().any(|p| &p.key == k) {
            return Err(Error::new(
                ErrorCode::TemplateParamUnknown,
                format!("模板 {} 未声明参数 {k:?}", entry.summary.id),
            ));
        }
        if v.trim().is_empty() {
            return Err(Error::new(
                ErrorCode::TemplateParamMissing,
                format!("参数 {k:?} 的值不能为空"),
            ));
        }
    }
    for p in &entry.params {
        if p.required && !params.contains_key(&p.key) {
            return Err(Error::new(
                ErrorCode::TemplateParamMissing,
                format!("缺少必填参数 {key:?}", key = p.key),
            ));
        }
    }
    Ok(())
}

/// 参数替换：UTF-8 文本文件内的 `{{key}}` → 值；非 UTF-8 内容原样跳过。
/// 替换发生在落盘前的内存副本上，失败不影响磁盘。
fn apply_params(files: &mut [(String, Vec<u8>)], params: &BTreeMap<String, String>) {
    if params.is_empty() {
        return;
    }
    for (_rel, bytes) in files.iter_mut() {
        let Ok(text) = std::str::from_utf8(bytes) else {
            continue;
        };
        let mut replaced = text.to_string();
        for (k, v) in params {
            let needle = format!("{{{{{k}}}}}");
            if replaced.contains(&needle) {
                replaced = replaced.replace(&needle, v);
            }
        }
        *bytes = replaced.into_bytes();
    }
}

/// 组合模板的生成计划：端口分配后的 services 片段 + 文件并集。
struct BlockPlan {
    /// (服务 id, 已写入端口的 services 片段)。
    services: Vec<(String, Value)>,
    /// 选中块的文件并集（清单顺序）。
    files: Vec<String>,
}

/// 组合选择 → 生成计划。`selected` 为 None 表示全块；
/// 依赖缺失 → TEMPLATE_BLOCK_DEP；端口缺失/重复 → TEMPLATE_BLOCK_PORT。
fn plan_blocks(
    entry: &TemplateEntry,
    selected: Option<&[String]>,
    ports: &BTreeMap<String, u32>,
) -> Result<Option<BlockPlan>> {
    if entry.blocks.is_empty() {
        return Ok(None);
    }
    // 依赖闭合：从显式选择出发，把 requires 递归并入（清单顺序稳定）
    let mut chosen: Vec<String> = match selected {
        Some(ids) => ids.to_vec(),
        None => entry.blocks.iter().map(|b| b.id.clone()).collect(),
    };
    for id in &chosen {
        if !entry.blocks.iter().any(|b| &b.id == id) {
            return Err(Error::new(
                ErrorCode::TemplateBlockDep,
                format!("块 {id:?} 在模板中不存在"),
            ));
        }
    }
    let mut i = 0;
    while i < chosen.len() {
        let requires = entry
            .blocks
            .iter()
            .find(|b| &b.id == &chosen[i])
            .map(|b| b.requires.clone())
            .unwrap_or_default();
        for r in requires {
            if !chosen.contains(&r) {
                chosen.push(r);
            }
        }
        i += 1;
    }
    let chosen_set: HashSet<&str> = chosen.iter().map(|s| s.as_str()).collect();

    let mut services: Vec<(String, Value)> = Vec::new();
    let mut used_ports: BTreeMap<u32, String> = BTreeMap::new();
    for b in &entry.blocks {
        if !chosen_set.contains(b.id.as_str()) {
            continue;
        }
        let mapping = b.services.as_mapping().expect("validate_blocks 已保证映射");
        for (k, item_value) in mapping {
            let svc_id = k
                .as_str()
                .expect("validate_blocks 已保证字符串键")
                .to_string();
            let port = match ports.get(&svc_id).copied() {
                Some(p) => p,
                None => b.default_port.ok_or_else(|| {
                    Error::new(
                        ErrorCode::TemplateBlockPort,
                        format!("服务 {svc_id:?} 未分配端口（块 {} 无 default_port）", b.id),
                    )
                })?,
            };
            if let Some(owner) = used_ports.get(&port) {
                return Err(Error::new(
                    ErrorCode::TemplateBlockPort,
                    format!("端口 {port} 同时分配给 {owner:?} 与 {svc_id:?}"),
                ));
            }
            used_ports.insert(port, svc_id.clone());
            // 片段里的 {{port}} 占位一并替换（如 health URL），再回填 port 字段
            let mut item = {
                let rendered = serde_yaml::to_string(&item_value).map_err(|e| {
                    Error::new(
                        ErrorCode::TemplateInvalid,
                        format!("块 {} services 序列化失败: {e}", b.id),
                    )
                })?;
                let replaced = rendered.replace("{{port}}", &port.to_string());
                serde_yaml::from_str::<Value>(&replaced).map_err(|e| {
                    Error::new(
                        ErrorCode::TemplateInvalid,
                        format!("块 {} services 占位替换失败: {e}", b.id),
                    )
                })?
            };
            if let Some(item_map) = item.as_mapping_mut() {
                item_map.insert(Value::from("port"), Value::from(port));
            }
            services.push((svc_id, item));
        }
    }

    let files: Vec<String> = block_files_union(
        &entry
            .blocks
            .iter()
            .filter(|b| chosen_set.contains(b.id.as_str()))
            .cloned()
            .collect::<Vec<_>>(),
    );
    Ok(Some(BlockPlan { services, files }))
}

/// 由块片段生成 supertask.yaml 文本：注入 templates 保留段与 apply_to 的 name 覆写。
fn build_blocks_workspace_yaml(
    entry: &TemplateEntry,
    plan: &BlockPlan,
    params: &BTreeMap<String, String>,
) -> Result<String> {
    let mut services = serde_yaml::Mapping::new();
    for (svc_id, item) in &plan.services {
        services.insert(Value::from(svc_id.as_str()), item.clone());
    }
    // 组合模板的 name：apply_to yaml.name 的参数优先，否则用模板名
    let mut name = entry.summary.name.clone();
    for p in &entry.params {
        if p.apply_to.iter().any(|t| t == "yaml.name") {
            if let Some(v) = params.get(&p.key) {
                name = v.clone();
            }
        }
    }
    let mut root = serde_yaml::Mapping::new();
    root.insert(Value::from("version"), Value::from(1));
    root.insert(Value::from("kind"), Value::from("workspace"));
    root.insert(Value::from("name"), Value::from(name.as_str()));
    root.insert(Value::from("root"), Value::from("."));
    root.insert(Value::from("services"), Value::Mapping(services));
    let mut section = serde_yaml::Mapping::new();
    section.insert(
        Value::from("source"),
        Value::from(entry.summary.source.as_str()),
    );
    section.insert(Value::from("id"), Value::from(entry.summary.id.as_str()));
    section.insert(
        Value::from("version"),
        Value::from(entry.summary.version.as_str()),
    );
    root.insert(Value::from("templates"), Value::Mapping(section));
    serde_yaml::to_string(&Value::Mapping(root)).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("组合模板 supertask.yaml 序列化失败: {e}"),
        )
    })
}

/// `templates.preview` 的纯计算：组合选择 → 将生成的 services / 文件清单 / 警告，
/// 无任何落盘副作用。与 create 走同一套 plan_blocks 校验。
pub fn preview_template(
    template_id: &str,
    source: TemplateSourceKind,
    local_dir: Option<&Path>,
    blocks: Option<&[String]>,
    ports: &BTreeMap<String, u32>,
    params: &BTreeMap<String, String>,
) -> Result<TemplatePreviewOut> {
    let entry = resolve_entry(template_id, source, local_dir)?;
    if entry.summary.invalid {
        return Err(Error::new(
            ErrorCode::TemplateInvalid,
            entry
                .summary
                .invalid_reason
                .unwrap_or_else(|| "模板清单损坏".into()),
        ));
    }
    validate_param_values(&entry, params)?;
    let plan = plan_blocks(&entry, blocks, ports)?;
    // 预览顺带校验文件集合与磁盘/嵌入一致（组合模板校验选中块并集）
    verify_entry_files(&entry)?;
    let mut warnings: Vec<String> = Vec::new();
    if let Some(sel) = blocks {
        for p in &entry.params {
            if p.required && !params.contains_key(&p.key) {
                warnings.push(format!("缺少必填参数 {:.0}", p.key));
            }
        }
        if plan.is_none() {
            warnings.push("该模板没有组合块，blocks 选择被忽略".into());
        }
        let _ = sel;
    }
    let services = match &plan {
        Some(plan) => {
            let mut m = serde_yaml::Mapping::new();
            for (svc_id, item) in &plan.services {
                m.insert(Value::from(svc_id.as_str()), item.clone());
            }
            Value::Mapping(m)
        }
        // 普通模板：预览 = 模板自带 supertask.yaml 的 services 段
        None => {
            let files = verify_entry_files(&entry)?;
            let yaml = files
                .iter()
                .find(|(rel, _)| rel == "supertask.yaml")
                .map(|(_, b)| b.as_slice())
                .ok_or_else(|| Error::new(ErrorCode::TemplateInvalid, "模板缺少 supertask.yaml"))?;
            let text = std::str::from_utf8(yaml).map_err(|_| {
                Error::new(ErrorCode::TemplateInvalid, "模板 supertask.yaml 不是 UTF-8")
            })?;
            let raw: Value = serde_yaml::from_str(text)
                .map_err(|e| Error::new(ErrorCode::TemplateInvalid, format!("解析失败: {e}")))?;
            raw.get("services").cloned().unwrap_or(Value::Null)
        }
    };
    Ok(TemplatePreviewOut {
        services,
        files: plan
            .map(|p| p.files)
            .unwrap_or_else(|| entry.summary.files.clone()),
        warnings,
    })
}

/// `templates.preview` 输出。
#[derive(Debug, Clone, Serialize)]
pub struct TemplatePreviewOut {
    /// 将生成的 `services` 映射（端口已分配）。
    pub services: Value,
    /// 将写入的模板文件（相对路径）。
    pub files: Vec<String>,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// 方向九：模板分享——导入（zip 包 → 本地库）与导出（本地/内置 → 可分享 zip）。
// 分享单元是 zip 文件（用户手动传递），不做远端分发；安全蓝本对齐数据快照
// （snapshot.rs 的条目上限与路径规则），失败不落盘半成品。
// ---------------------------------------------------------------------------

/// 导入包条目上限（模板规模远小于数据快照的 2 万条 / 512 MiB）。
pub const MAX_TEMPLATE_PACKAGE_ENTRIES: usize = 2_000;
/// 导入包总字节上限。
pub const MAX_TEMPLATE_PACKAGE_TOTAL_BYTES: u64 = 64 * 1024 * 1024;

/// `templates.import` 输出。
#[derive(Debug, Clone, Serialize)]
pub struct TemplateImportOut {
    pub id: String,
    /// 落盘的模板文件数（不含 template.yaml）。
    pub files: usize,
}

/// 校验模板 id 可安全作为本地库目录名：单段、仅 ASCII 字母数字/连字符/下划线、
/// 最长 64。`parse_manifest` 只校验 id == 目录名，不约束字符集；导入的 id 来自
/// 不受信 zip 且直接成为目录名，必须先过这道关。
fn validate_import_id(id: &str) -> Result<()> {
    let ok = !id.is_empty()
        && id.len() <= 64
        && !id.starts_with('-')
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板 id {id:?} 非法：仅允许字母数字/连字符/下划线，最长 64"),
        ))
    }
}

/// 导入包内条目的相对路径安全规则：禁 `..`/`.`/空段/反斜杠/冒号/隐藏段，且
/// 拒绝构建产物目录段（与读取侧 collect_local_files 的跳过口径一致，避免导入后
/// 清单⇄目录双向一致性被破坏）。
fn safe_package_rel(p: &str) -> Option<String> {
    if p.is_empty() || p.contains('\\') || p.contains(':') || p.starts_with('/') {
        return None;
    }
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        if seg.is_empty() || seg == "." || seg == ".." || seg.starts_with('.') {
            return None;
        }
        if SKIP_DIRS.contains(&seg) {
            return None;
        }
        out.push(seg);
    }
    if out.is_empty() {
        None
    } else {
        Some(out.join("/"))
    }
}

/// 读取 zip 包内全部文件条目为 (路径, 字节)。目录条目跳过；条目数/总字节超
/// 上限、路径不安全 → `TEMPLATE_INVALID`。
fn read_package(zip_path: &Path) -> Result<Vec<(String, Vec<u8>)>> {
    use std::io::Read as _;
    if !zip_path.is_file() {
        return Err(Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板包不存在: {}", zip_path.display()),
        ));
    }
    let file = fs::File::open(zip_path)
        .map_err(|e| Error::new(ErrorCode::TemplateInvalid, format!("模板包不可读: {e}")))?;
    let mut archive = zip::ZipArchive::new(file)
        .map_err(|e| Error::new(ErrorCode::TemplateInvalid, format!("模板包打开失败: {e}")))?;
    let mut out: Vec<(String, Vec<u8>)> = Vec::new();
    let mut total = 0u64;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| Error::new(ErrorCode::TemplateInvalid, format!("模板包条目损坏: {e}")))?;
        if entry.is_dir() {
            continue;
        }
        let raw = entry.name().to_string();
        let Some(rel) = safe_package_rel(&raw) else {
            return Err(Error::new(
                ErrorCode::TemplateInvalid,
                format!("模板包含不安全条目路径: {raw:?}"),
            ));
        };
        if out.len() >= MAX_TEMPLATE_PACKAGE_ENTRIES {
            return Err(Error::new(
                ErrorCode::TemplateInvalid,
                format!("模板包条目数超上限 {MAX_TEMPLATE_PACKAGE_ENTRIES}"),
            ));
        }
        let mut bytes = Vec::with_capacity(entry.size() as usize);
        entry
            .read_to_end(&mut bytes)
            .map_err(|e| Error::new(ErrorCode::TemplateInvalid, format!("模板包读取失败: {e}")))?;
        total += bytes.len() as u64;
        if total > MAX_TEMPLATE_PACKAGE_TOTAL_BYTES {
            return Err(Error::new(
                ErrorCode::TemplateInvalid,
                format!("模板包总字节超上限 {MAX_TEMPLATE_PACKAGE_TOTAL_BYTES}"),
            ));
        }
        out.push((rel, bytes));
    }
    if out.is_empty() {
        return Err(Error::new(ErrorCode::TemplateInvalid, "模板包为空"));
    }
    Ok(out)
}

/// 归一化包内模板根：清单在包根，或恰有一个含清单的顶层目录（直接压缩模板
/// 目录的常见形态）。返回 (根前缀, 清单字节)；其余布局一律拒绝。
fn package_root(files: &[(String, Vec<u8>)]) -> Result<(String, Vec<u8>)> {
    let manifest_at = |prefix: String| {
        files
            .iter()
            .find(|(p, _)| *p == format!("{prefix}{MANIFEST_FILE}"))
            .map(|(_, b)| b.clone())
    };
    if let Some(bytes) = manifest_at(String::new()) {
        return Ok((String::new(), bytes));
    }
    let tops: HashSet<&str> = files
        .iter()
        .map(|(p, _)| p.split('/').next().unwrap_or(""))
        .collect();
    let with_manifest: Vec<&&str> = tops
        .iter()
        .filter(|t| manifest_at(format!("{t}/")).is_some())
        .collect();
    match with_manifest.as_slice() {
        [top] => {
            let prefix = format!("{top}/");
            let bytes = manifest_at(prefix.clone()).unwrap_or_default();
            Ok((prefix, bytes))
        }
        [] => Err(Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板包缺少 {MANIFEST_FILE}"),
        )),
        _ => Err(Error::new(
            ErrorCode::TemplateInvalid,
            "模板包存在多个含清单的顶层目录，无法定位模板根",
        )),
    }
}

/// 导入模板包到本地库（方向九模板分享的最小写入路径）。
///
/// 全量校验通过后才落盘：先解包到 local_dir 下的隐藏 staging 目录，再原子改名
/// 为 `<id>/`；任一步失败清理 staging，不产生半成品。
pub fn import_template(zip_path: &Path, local_dir: &Path) -> Result<TemplateImportOut> {
    let packaged = read_package(zip_path)?;
    let (prefix, manifest_bytes) = package_root(&packaged)?;
    if !prefix.is_empty() {
        let outside = packaged.iter().find(|(p, _)| !p.starts_with(&prefix));
        if let Some((p, _)) = outside {
            return Err(Error::new(
                ErrorCode::TemplateInvalid,
                format!("模板包含模板根之外的条目: {p:?}"),
            ));
        }
    }
    let strip = |p: &str| p.strip_prefix(&prefix).unwrap_or(p).to_string();
    let manifest_text = std::str::from_utf8(&manifest_bytes)
        .map_err(|_| Error::new(ErrorCode::TemplateInvalid, "template.yaml 不是 UTF-8"))?;
    // 清单 id 先于 parse_manifest 取出：它决定目标目录名，且 parse_manifest 的
    // id==目录名校验以它为参数
    let raw: Value = serde_yaml::from_str(manifest_text).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("template.yaml 解析失败: {e}"),
        )
    })?;
    let id = raw
        .get("id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::new(ErrorCode::TemplateInvalid, "template.yaml 缺少 id"))?;
    validate_import_id(id)?;

    let staged = local_dir.join(id);
    if builtin_ids().contains(id) {
        return Err(Error::new(
            ErrorCode::TemplateIdConflict,
            format!("模板 id {id:?} 与内置模板冲突"),
        ));
    }
    if staged.exists() {
        return Err(Error::new(
            ErrorCode::TemplateIdConflict,
            format!("本地模板 {id:?} 已存在"),
        ));
    }

    let manifest = parse_manifest(id, &manifest_bytes)?;
    // 清单⇄包内容双向一致（口径对齐 verify_entry_files，但在内存中先验，不落盘）
    let mut actual: Vec<String> = packaged
        .iter()
        .map(|(p, _)| strip(p))
        .filter(|p| *p != MANIFEST_FILE)
        .collect();
    actual.sort();
    let mut expected_owned = if manifest.blocks.is_empty() {
        manifest.files.clone()
    } else {
        block_files_union(&manifest.blocks)
    };
    expected_owned.sort();
    if actual != expected_owned {
        let missing: Vec<&String> = expected_owned
            .iter()
            .filter(|e| !actual.contains(e))
            .collect();
        let extra: Vec<&String> = actual
            .iter()
            .filter(|a| !expected_owned.contains(a))
            .collect();
        return Err(Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板包文件与清单不一致：缺少 {missing:?}，多余 {extra:?}"),
        ));
    }

    fs::create_dir_all(local_dir)
        .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("本地模板目录不可用: {e}")))?;
    let staging = local_dir.join(format!(
        ".import-staging-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    let install = (|| -> Result<()> {
        fs::create_dir_all(staging.join(id)).map_err(|e| {
            Error::new(
                ErrorCode::TemplateWrite,
                format!("无法创建 staging 目录: {e}"),
            )
        })?;
        write_file(&staging.join(id), MANIFEST_FILE, &manifest_bytes)?;
        for (path, bytes) in &packaged {
            let rel = strip(path);
            if rel == MANIFEST_FILE {
                continue;
            }
            write_file(&staging.join(id), &rel, bytes)?;
        }
        fs::rename(staging.join(id), &staged).map_err(|e| {
            Error::new(
                ErrorCode::TemplateWrite,
                format!("无法安装模板 {id:?}: {e}"),
            )
        })?;
        Ok(())
    })();
    let _ = fs::remove_dir_all(&staging);
    install?;
    Ok(TemplateImportOut {
        id: id.to_string(),
        files: packaged.len() - 1,
    })
}

/// 导出模板为可分享 zip 包（方向九）：包根含 template.yaml 与全部模板文件，
/// 可直接被 `import_template` 导回。返回生成的包路径。
pub fn export_template(
    template_id: &str,
    source: TemplateSourceKind,
    target_dir: &Path,
    local_dir: Option<&Path>,
) -> Result<PathBuf> {
    let entry = resolve_entry(template_id, source, local_dir)?;
    if !target_dir.is_dir() {
        return Err(Error::new(
            ErrorCode::NotFound,
            format!("目标目录不存在: {}", target_dir.display()),
        ));
    }
    let verified = verify_entry_files(&entry)?;
    // 清单字节：local 直接读；builtin 从嵌入资源按后缀定位（路径可能带 id 前缀）
    let manifest_bytes: Vec<u8> = match &entry.kind {
        TemplateKind::Local(root) => fs::read(root.join(MANIFEST_FILE))
            .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("无法读取清单: {e}")))?,
        TemplateKind::Builtin(dir) => {
            let mut embedded: Vec<(String, &'static [u8])> = Vec::new();
            collect_embedded_files(dir, &mut embedded);
            let suffix = format!("/{MANIFEST_FILE}");
            embedded
                .iter()
                .find(|(p, _)| p == MANIFEST_FILE || p.ends_with(&suffix))
                .map(|(_, b)| b.to_vec())
                .ok_or_else(|| {
                    Error::new(ErrorCode::TemplateInvalid, "内置模板缺少清单".to_string())
                })?
        }
    };

    let out_path = target_dir.join(format!("{}-template.zip", entry.summary.id));
    if out_path.exists() {
        return Err(Error::new(
            ErrorCode::TargetNotEmpty,
            format!("目标已存在: {}", out_path.display()),
        ));
    }
    let tmp_path = out_path.with_extension("zip.part");
    let write = (|| -> Result<()> {
        use std::io::Write as _;
        let file = fs::File::create(&tmp_path)
            .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("无法创建包文件: {e}")))?;
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut put = |name: &str, bytes: &[u8]| -> Result<()> {
            w.start_file(name, opts).map_err(|e| {
                Error::new(ErrorCode::TemplateWrite, format!("写入 {name} 失败: {e}"))
            })?;
            w.write_all(bytes).map_err(|e| {
                Error::new(ErrorCode::TemplateWrite, format!("写入 {name} 失败: {e}"))
            })?;
            Ok(())
        };
        put(MANIFEST_FILE, &manifest_bytes)?;
        for (rel, bytes) in &verified {
            put(rel, bytes)?;
        }
        w.finish()
            .map_err(|e| Error::new(ErrorCode::TemplateWrite, format!("包收尾失败: {e}")))?;
        Ok(())
    })();
    if let Err(e) = write {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }
    fs::rename(&tmp_path, &out_path).map_err(|e| {
        let _ = fs::remove_file(&tmp_path);
        Error::new(ErrorCode::TemplateWrite, format!("无法落盘: {e}"))
    })?;
    Ok(out_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-tpl-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// 在本地模板根下写一套最小可用的 local 模板。
    fn write_local_template(root: &Path, id: &str, manifest_extra: &str) {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("supertask.yaml"),
            format!("version: 1\nname: {id}\nservices:\n  {id}:\n    kind: node\n    dir: .\n    port: 3111\n"),
        )
        .unwrap();
        fs::write(dir.join("README.md"), format!("# {id}\n")).unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            format!(
                "id: {id}\nversion: \"1\"\nname: 模板 {id}\ndescription: 本地测试模板\nstacks:\n  - node\nfiles:\n  - supertask.yaml\n  - README.md\n{manifest_extra}"
            ),
        )
        .unwrap();
    }

    // ---- 方向九：模板导入 / 导出 ----

    /// 在内存里组一个模板 zip 包。
    fn build_zip(tag: &str, entries: &[(&str, Vec<u8>)]) -> PathBuf {
        use std::io::Write as _;
        let path = temp_dir(tag).join("pkg.zip");
        let file = fs::File::create(&path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default();
        for (name, bytes) in entries {
            w.start_file(*name, opts).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
        path
    }

    fn manifest_yaml(id: &str) -> Vec<u8> {
        format!(
            "id: {id}\nversion: \"1\"\nname: 模板 {id}\ndescription: 测试导入\nstacks:\n  - node\nfiles:\n  - supertask.yaml\n  - README.md\n"
        )
        .into_bytes()
    }

    fn stub_yaml(name: &str) -> Vec<u8> {
        format!("version: 1\nname: {name}\nservices:\n  web:\n    kind: node\n    dir: .\n    port: 3222\n")
            .into_bytes()
    }

    #[test]
    fn import_export_roundtrip() {
        let lib_a = temp_dir("lib-a");
        let lib_b = temp_dir("lib-b");
        let target = temp_dir("export-target");
        write_local_template(&lib_a, "share-me", "");
        let zip_path =
            export_template("share-me", TemplateSourceKind::Local, &target, Some(&lib_a)).unwrap();
        assert_eq!(
            zip_path.file_name().unwrap().to_str().unwrap(),
            "share-me-template.zip"
        );

        let out = import_template(&zip_path, &lib_b).unwrap();
        assert_eq!(out.id, "share-me");
        assert_eq!(out.files, 2); // supertask.yaml + README.md（不含清单）

        let list = list_templates(Some(&lib_b));
        let t = list
            .iter()
            .find(|t| t.id == "share-me")
            .expect("导入后可见");
        assert!(!t.invalid);
        assert_eq!(t.source, TemplateSourceKind::Local);
        // 清单声明顺序（非排序）
        assert_eq!(t.files, vec!["supertask.yaml", "README.md"]);
    }

    #[test]
    fn export_builtin_reimport_conflicts() {
        // 内置随应用分发：同 id 本地库条目被禁止（防遮蔽），因此内置包导回必然冲突。
        // 导出内置的价值是「以此为起点改造后以新 id 分享」。
        let mut builtins = list_templates(None);
        let some = builtins.remove(0);
        let target = temp_dir("exp-builtin");
        let zip_path =
            export_template(&some.id, TemplateSourceKind::Builtin, &target, None).unwrap();
        let lib = temp_dir("lib-builtin");
        let e = import_template(&zip_path, &lib).unwrap_err();
        assert_eq!(e.code(), ErrorCode::TemplateIdConflict);
    }

    #[test]
    fn import_accepts_wrapped_root_form() {
        let lib = temp_dir("lib-wrapped");
        let zip_path = build_zip(
            "w1",
            &[
                ("wrapped/template.yaml", manifest_yaml("wrapped")),
                ("wrapped/supertask.yaml", stub_yaml("wrapped")),
                ("wrapped/README.md", b"# wrapped\n".to_vec()),
            ],
        );
        let out = import_template(&zip_path, &lib).unwrap();
        assert_eq!(out.id, "wrapped");
        assert_eq!(out.files, 2);
        assert!(lib.join("wrapped").join("supertask.yaml").is_file());
        // staging 已清理，根下只有模板目录
        let leftovers: Vec<_> = fs::read_dir(&lib).unwrap().flatten().collect();
        assert_eq!(leftovers.len(), 1);
    }

    #[test]
    fn import_rejects_conflicts() {
        let lib = temp_dir("lib-conflict");
        // 与内置 id 冲突（冲突检查先于文件一致性）
        let builtin_id = list_templates(None).remove(0).id;
        let m = manifest_yaml(&builtin_id);
        let zip1 = build_zip("c1", &[("template.yaml", m)]);
        assert_eq!(
            import_template(&zip1, &lib).unwrap_err().code(),
            ErrorCode::TemplateIdConflict
        );
        // 与现有本地模板冲突
        write_local_template(&lib, "dup", "");
        let target = temp_dir("exp-dup");
        let zip2 = export_template("dup", TemplateSourceKind::Local, &target, Some(&lib)).unwrap();
        assert_eq!(
            import_template(&zip2, &lib).unwrap_err().code(),
            ErrorCode::TemplateIdConflict
        );
    }

    #[test]
    fn import_rejects_unsafe_content() {
        let lib = temp_dir("lib-unsafe");
        // id 字符集非法（路径逃逸形态）
        let m = manifest_yaml("../evil");
        let bad_id = build_zip("u1", &[("template.yaml", m)]);
        assert_eq!(
            import_template(&bad_id, &lib).unwrap_err().code(),
            ErrorCode::TemplateInvalid
        );
        // 缺清单
        let no_manifest = build_zip("u2", &[("a.txt", b"x".to_vec())]);
        assert_eq!(
            import_template(&no_manifest, &lib).unwrap_err().code(),
            ErrorCode::TemplateInvalid
        );
        // 包内不安全条目路径
        let m = manifest_yaml("ok-id");
        let unsafe_entry = build_zip(
            "u3",
            &[("template.yaml", m), ("../escape.txt", b"x".to_vec())],
        );
        assert_eq!(
            import_template(&unsafe_entry, &lib).unwrap_err().code(),
            ErrorCode::TemplateInvalid
        );
        // 清单与内容不一致（README.md 缺失）
        let m = manifest_yaml("ok-id");
        let mismatch = build_zip(
            "u4",
            &[("template.yaml", m), ("supertask.yaml", stub_yaml("ok"))],
        );
        assert_eq!(
            import_template(&mismatch, &lib).unwrap_err().code(),
            ErrorCode::TemplateInvalid
        );
        // 多余的清单外文件
        let m = manifest_yaml("ok-id");
        let extra = build_zip(
            "u5",
            &[
                ("template.yaml", m),
                ("supertask.yaml", stub_yaml("ok")),
                ("README.md", b"x".to_vec()),
                ("stowaway.txt", b"x".to_vec()),
            ],
        );
        assert_eq!(
            import_template(&extra, &lib).unwrap_err().code(),
            ErrorCode::TemplateInvalid
        );
        // 源不存在
        assert_eq!(
            import_template(&lib.join("absent.zip"), &lib)
                .unwrap_err()
                .code(),
            ErrorCode::TemplateInvalid
        );
    }

    #[test]
    fn export_target_rules() {
        let lib = temp_dir("lib-export");
        write_local_template(&lib, "exp-me", "");
        // 目标目录不存在
        let e = export_template(
            "exp-me",
            TemplateSourceKind::Local,
            &temp_dir("exp-missing").join("nope"),
            Some(&lib),
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::NotFound);
        // 目标 zip 已存在
        let target = temp_dir("exp-target");
        export_template("exp-me", TemplateSourceKind::Local, &target, Some(&lib)).unwrap();
        let e2 =
            export_template("exp-me", TemplateSourceKind::Local, &target, Some(&lib)).unwrap_err();
        assert_eq!(e2.code(), ErrorCode::TargetNotEmpty);
    }

    #[test]
    fn builtin_manifests_match_embedded_assets() {
        let entries = builtin_entries();
        assert!(entries.len() >= 2);
        for entry in &entries {
            assert!(
                !entry.summary.invalid,
                "内置模板清单损坏: {:?}",
                entry.summary.invalid_reason
            );
            let actual = verify_entry_files(entry)
                .unwrap_or_else(|e| panic!("模板 {} 校验失败: {e}", entry.summary.id));
            // files 概览与实际文件集合完全一致（双向）
            let mut expected: Vec<&str> = entry.summary.files.iter().map(|s| s.as_str()).collect();
            expected.sort_unstable();
            let mut actual_paths: Vec<&str> = actual.iter().map(|(p, _)| p.as_str()).collect();
            actual_paths.sort_unstable();
            assert_eq!(
                expected, actual_paths,
                "模板 {} files 概览不一致",
                entry.summary.id
            );
            assert_eq!(entry.summary.source, TemplateSourceKind::Builtin);
        }
        // 嵌入根下不允许出现清单之外的散落模板目录
        let known: HashSet<String> = entries.iter().map(|e| e.summary.id.clone()).collect();
        for dir in TEMPLATE_ASSETS.dirs() {
            let name = dir
                .path()
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned();
            assert!(
                known.contains(&name),
                "嵌入目录 {name} 缺少 template.yaml 清单"
            );
        }
    }

    #[test]
    fn manifests_do_not_list_themselves() {
        for entry in builtin_entries() {
            assert!(
                !entry.summary.files.iter().any(|f| f == MANIFEST_FILE),
                "模板 {} 的 files 不应包含清单自身",
                entry.summary.id
            );
        }
    }

    #[test]
    fn rejects_bad_directory_names() {
        let parent = std::env::temp_dir();
        let bad = [
            "",
            ".",
            "..",
            "a/b",
            "a\\b",
            "../x",
            "..\\x",
            "C:\\x",
            "C:x",
            "a:b",
            r"\\server\share",
            "//server/share",
            r"\\?\C:\x",
            "CON",
            "aux",
            "Nul.txt",
            "foo.",
            "foo ",
            "a<b",
        ];
        for name in bad {
            let err = create_template(
                "spring-multimodule-node",
                TemplateSourceKind::Builtin,
                &parent,
                name,
                None,
                &BTreeMap::new(),
                None,
                &BTreeMap::new(),
            )
            .unwrap_err();
            assert_eq!(
                err.code(),
                ErrorCode::PathEscape,
                "目录名 {name:?} 应被拒绝"
            );
        }
        for ok in ["demo-app", "my_workspace", "项目01", "a.b.c"] {
            validate_directory_name(ok).unwrap_or_else(|e| panic!("目录名 {ok:?} 应合法: {e}"));
        }
    }

    #[test]
    fn unknown_template_id_not_found() {
        let parent = temp_dir("unknown");
        let err = create_template(
            "no-such-template",
            TemplateSourceKind::Builtin,
            &parent,
            "demo",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn missing_parent_is_cwd_missing() {
        let missing = std::env::temp_dir()
            .join(format!("st-tpl-missing-{}", std::process::id()))
            .join("no-such-parent");
        let err = create_template(
            "spring-multimodule-node",
            TemplateSourceKind::Builtin,
            &missing,
            "demo",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::CwdMissing);
    }

    #[test]
    fn creates_new_directory_and_yaml_has_templates_section() {
        let parent = temp_dir("create");
        let target = create_template(
            "spring-multimodule-node",
            TemplateSourceKind::Builtin,
            &parent,
            "demo-app",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(target.is_dir());
        assert!(target.ends_with("demo-app"));

        for rel in [
            "pom.xml",
            "backend/pom.xml",
            "backend/src/main/java/com/supertask/demo/DemoApplication.java",
            "backend/src/main/resources/application.properties",
            "web/package.json",
            "web/server.js",
            "supertask.yaml",
        ] {
            assert!(
                target
                    .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                    .is_file(),
                "缺少 {rel}"
            );
        }
        // 清单不应被复制到目标工作区
        assert!(!target.join(MANIFEST_FILE).exists());

        let text = fs::read_to_string(target.join("supertask.yaml")).unwrap();
        let (file, warnings) = parse_yaml(&text).unwrap();
        assert!(warnings.is_empty(), "模板 YAML 不应产生告警: {warnings:?}");
        let tpl = file.templates.as_ref().expect("templates 段缺失");
        let m = tpl.as_mapping().expect("templates 应为映射");
        let get = |k: &str| m.get(Value::from(k)).and_then(|v| v.as_str()).unwrap();
        assert_eq!(get("source"), "builtin");
        assert_eq!(get("id"), "spring-multimodule-node");
        assert_eq!(get("version"), "1");

        assert_eq!(file.services.len(), 2);
        let web = file.services.get("web").unwrap();
        assert_eq!(web.depends_on, vec!["backend"]);
        assert!(file.services.contains_key("backend"));
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn empty_existing_target_allowed() {
        let parent = temp_dir("empty");
        fs::create_dir_all(parent.join("dst")).unwrap();
        let target = create_template(
            "spring-multimodule-node",
            TemplateSourceKind::Builtin,
            &parent,
            "dst",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(target.join("pom.xml").is_file());
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn non_empty_target_rejected_and_untouched() {
        let parent = temp_dir("nonempty");
        let dst = parent.join("dst");
        fs::create_dir_all(dst.join("keep-dir")).unwrap();
        fs::write(dst.join("keep.txt"), "原样内容").unwrap();
        fs::write(dst.join("keep-dir/nested.txt"), "嵌套").unwrap();

        let err = create_template(
            "spring-multimodule-node",
            TemplateSourceKind::Builtin,
            &parent,
            "dst",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TargetNotEmpty);

        // 原目录内容一字未动，也没有混入模板文件
        assert_eq!(
            fs::read_to_string(dst.join("keep.txt")).unwrap(),
            "原样内容"
        );
        assert_eq!(
            fs::read_to_string(dst.join("keep-dir").join("nested.txt")).unwrap(),
            "嵌套"
        );
        let entries: Vec<String> = fs::read_dir(&dst)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(entries.len(), 2, "目标目录不应被修改: {entries:?}");
        assert!(!dst.join("pom.xml").exists());
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn minimal_yaml_omits_health_and_engine_fills_defaults() {
        // 模板自带 yaml 精简：backend 不写 health，交给 apply_defaults 兜底
        let entries = builtin_entries();
        let minimal = entries
            .iter()
            .find(|e| e.summary.id == "spring-multimodule-node-minimal")
            .unwrap();
        let files = verify_entry_files(minimal).unwrap();
        let (_, yaml_bytes) = files.iter().find(|(p, _)| p == "supertask.yaml").unwrap();
        let text = std::str::from_utf8(yaml_bytes).unwrap();
        // 结构化断言：模板自带 yaml 的 backend 不含 health 键（注释里的文字不算）
        let raw: Value = serde_yaml::from_str(text).unwrap();
        assert!(
            raw.get("services")
                .and_then(|s| s.get("backend"))
                .unwrap()
                .get("health")
                .is_none(),
            "最小模板 backend 不应自带 health 字段"
        );

        let mut file: SuperTaskFile = serde_yaml::from_str(text).unwrap();
        assert!(file.services.get("backend").unwrap().health.is_none());
        file.apply_defaults();
        assert!(file.services.get("backend").unwrap().health.is_some());
    }

    // ---------------- local 模板 ----------------

    #[test]
    fn local_template_listed_and_creatable() {
        let local_root = temp_dir("local-root");
        let parent = temp_dir("local-create");
        write_local_template(&local_root, "my-node-api", "");

        let list = list_templates(Some(&local_root));
        let mine = list
            .iter()
            .find(|t| t.id == "my-node-api")
            .expect("local 模板应被列出");
        assert_eq!(mine.source, TemplateSourceKind::Local);
        assert!(!mine.invalid);
        // builtin 仍在
        assert!(list.iter().any(|t| t.id == "spring-multimodule-node"));

        let target = create_template(
            "my-node-api",
            TemplateSourceKind::Local,
            &parent,
            "demo-local",
            Some(&local_root),
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        assert!(target.join("supertask.yaml").is_file());
        assert!(target.join("README.md").is_file());
        assert!(!target.join(MANIFEST_FILE).exists(), "清单不应复制到工作区");

        let text = fs::read_to_string(target.join("supertask.yaml")).unwrap();
        let (file, _) = parse_yaml(&text).unwrap();
        let tpl = file.templates.as_ref().expect("templates 段缺失");
        let m = tpl.as_mapping().unwrap();
        let get = |k: &str| m.get(Value::from(k)).and_then(|v| v.as_str()).unwrap();
        assert_eq!(get("source"), "local");
        assert_eq!(get("id"), "my-node-api");
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn local_id_conflict_skipped_in_list_and_rejected_on_create() {
        let local_root = temp_dir("conflict-root");
        let parent = temp_dir("conflict-create");
        // 与 builtin 同 id 的 local 模板：list 跳过，create 拒绝
        write_local_template(&local_root, "spring-multimodule-node", "");

        let list = list_templates(Some(&local_root));
        assert!(
            !list.iter().any(|t| t.id == "spring-multimodule-node" && t.source == TemplateSourceKind::Local),
            "冲突的 local 模板不应出现在 list"
        );

        let err = create_template(
            "spring-multimodule-node",
            TemplateSourceKind::Local,
            &parent,
            "demo",
            Some(&local_root),
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateIdConflict);
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn local_invalid_manifest_listed_as_invalid_and_create_rejected() {
        let local_root = temp_dir("invalid-root");
        let parent = temp_dir("invalid-create");
        let dir = local_root.join("broken-tpl");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(MANIFEST_FILE), "name: 缺少 id 与 files\n").unwrap();

        let list = list_templates(Some(&local_root));
        let broken = list
            .iter()
            .find(|t| t.id == "broken-tpl")
            .expect("损坏条目应列出");
        assert!(broken.invalid);
        assert!(broken
            .invalid_reason
            .as_deref()
            .unwrap_or_default()
            .contains("template.yaml"));

        let err = create_template(
            "broken-tpl",
            TemplateSourceKind::Local,
            &parent,
            "demo",
            Some(&local_root),
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateInvalid);
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn local_missing_files_rejected_before_write() {
        let local_root = temp_dir("stale-root");
        let parent = temp_dir("stale-create");
        write_local_template(&local_root, "stale-tpl", "");
        // 清单声明了不存在于目录的文件 → 创建前校验失败，目标目录保持为空
        let manifest = format!(
            "id: stale-tpl\nversion: \"1\"\nname: 过期清单\nstacks:\n  - node\nfiles:\n  - supertask.yaml\n  - README.md\n  - ghost.txt\n"
        );
        fs::write(local_root.join("stale-tpl").join(MANIFEST_FILE), manifest).unwrap();

        let target = parent.join("dst");
        fs::create_dir_all(&target).unwrap();
        let err = create_template(
            "stale-tpl",
            TemplateSourceKind::Local,
            &parent,
            "dst",
            Some(&local_root),
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateInvalid);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0, "失败不应落盘");
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn local_unknown_id_not_found_without_local_dir() {
        let parent = temp_dir("no-local-dir");
        let err = create_template(
            "whatever",
            TemplateSourceKind::Local,
            &parent,
            "demo",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn manifest_rejects_path_escape_in_files() {
        let err = parse_manifest(
            "escape",
            b"id: escape\nversion: \"1\"\nname: x\nfiles:\n  - ../evil.txt\n",
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateInvalid);
    }

    #[test]
    fn params_substitute_files_and_yaml_name() {
        let local_root = temp_dir("params-root");
        let parent = temp_dir("params-create");
        let dir = local_root.join("param-tpl");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("supertask.yaml"),
            "version: 1\nname: placeholder-name\nservices:\n  app:\n    kind: node\n    dir: .\n    port: 3111\n",
        )
        .unwrap();
        fs::write(dir.join("README.md"), "# {{project_name}}\n").unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            "id: param-tpl\nversion: \"1\"\nname: 参数模板\ndescription: x\nstacks:\n  - node\nfiles:\n  - supertask.yaml\n  - README.md\nparams:\n  - key: project_name\n    label: 项目名\n    required: true\n    apply_to:\n      - yaml.name\n",
        )
        .unwrap();

        let list = list_templates(Some(&local_root));
        let tpl = list.iter().find(|t| t.id == "param-tpl").unwrap();
        assert_eq!(tpl.params.as_ref().unwrap()[0].key, "project_name");

        let mut params = BTreeMap::new();
        params.insert("project_name".to_string(), "my-app".to_string());
        let target = create_template(
            "param-tpl",
            TemplateSourceKind::Local,
            &parent,
            "demo-params",
            Some(&local_root),
            &params,
            None,
            &BTreeMap::new(),
        )
        .unwrap();

        assert_eq!(
            fs::read_to_string(target.join("README.md")).unwrap(),
            "# my-app\n"
        );
        let (file, _) =
            parse_yaml(&fs::read_to_string(target.join("supertask.yaml")).unwrap()).unwrap();
        assert_eq!(
            file.name.as_deref(),
            Some("my-app"),
            "apply_to yaml.name 应覆写 name"
        );
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn params_missing_required_and_unknown_rejected() {
        let local_root = temp_dir("params-err-root");
        let parent = temp_dir("params-err-create");
        let dir = local_root.join("req-tpl");
        fs::create_dir_all(&dir).unwrap();
        fs::write(
            dir.join("supertask.yaml"),
            "version: 1\nname: req\nservices:\n  app:\n    kind: node\n    dir: .\n    port: 3111\n",
        )
        .unwrap();
        fs::write(
            dir.join(MANIFEST_FILE),
            "id: req-tpl\nversion: \"1\"\nname: 必填模板\ndescription: x\nstacks:\n  - node\nfiles:\n  - supertask.yaml\nparams:\n  - key: project_name\n    label: 项目名\n    required: true\n    apply_to:\n      - yaml.name\n",
        )
        .unwrap();

        // 缺 required → TEMPLATE_PARAM_MISSING（不落盘）
        let target = parent.join("dst1");
        fs::create_dir_all(&target).unwrap();
        let err = create_template(
            "req-tpl",
            TemplateSourceKind::Local,
            &parent,
            "dst1",
            Some(&local_root),
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateParamMissing);
        assert_eq!(fs::read_dir(&target).unwrap().count(), 0);

        // 未声明参数 → TEMPLATE_PARAM_UNKNOWN
        let mut extra = BTreeMap::new();
        extra.insert("project_name".to_string(), "x".to_string());
        extra.insert("hacker".to_string(), "y".to_string());
        let err = create_template(
            "req-tpl",
            TemplateSourceKind::Local,
            &parent,
            "dst1",
            Some(&local_root),
            &extra,
            None,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateParamUnknown);
        let _ = fs::remove_dir_all(&local_root);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn manifest_rejects_unknown_apply_to_target() {
        let err = parse_manifest(
            "bad-apply",
            b"id: bad-apply\nversion: \"1\"\nname: x\nfiles:\n  - a.txt\nparams:\n  - key: k\n    apply_to:\n      - yaml.description\n",
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateInvalid);
    }

    // ---------------- 组合模板（blocks） ----------------

    fn combo_entry() -> TemplateEntry {
        builtin_entries()
            .into_iter()
            .find(|e| e.summary.id == "spring-node-combo")
            .expect("内置组合模板 spring-node-combo 应存在")
    }

    #[test]
    fn combo_template_summary_exposes_blocks() {
        let entry = combo_entry();
        let blocks = entry
            .summary
            .blocks
            .as_ref()
            .expect("组合模板应暴露 blocks");
        assert_eq!(blocks.len(), 2);
        let web = blocks.iter().find(|b| b.id == "web").unwrap();
        assert_eq!(web.requires, vec!["backend"]);
        assert_eq!(web.default_port, Some(5173));
        assert_eq!(web.services, vec!["web"]);
        // 概览 files = 块文件并集
        assert!(entry.summary.files.contains(&"pom.xml".to_string()));
        assert!(entry.summary.files.contains(&"web/server.js".to_string()));
    }

    #[test]
    fn combo_preview_default_all_blocks_with_default_ports() {
        let out = preview_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            None,
            None,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let services = out.services.as_mapping().unwrap();
        assert_eq!(services.len(), 2);
        let backend = services.get(Value::from("backend")).unwrap();
        assert_eq!(
            backend.get("port").and_then(|p| p.as_u64()),
            Some(8081),
            "未分配端口时用 default_port"
        );
        // {{port}} 占位已替换进 health URL
        let url = backend
            .get("health")
            .unwrap()
            .get("http")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(url, "http://127.0.0.1:8081/actuator/health");
        assert!(out.files.iter().any(|f| f == "web/server.js"));
    }

    #[test]
    fn combo_dependency_auto_closed_and_port_overridden() {
        // 只选 web → 自动带上 backend；web 指定端口覆写 default
        let mut ports = BTreeMap::new();
        ports.insert("web".to_string(), 6000u32);
        let out = preview_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            None,
            Some(&["web".to_string()]),
            &ports,
            &BTreeMap::new(),
        )
        .unwrap();
        let services = out.services.as_mapping().unwrap();
        assert_eq!(services.len(), 2, "依赖 backend 应自动闭合");
        assert_eq!(
            services
                .get(Value::from("web"))
                .unwrap()
                .get("port")
                .and_then(|p| p.as_u64()),
            Some(6000)
        );
        assert_eq!(
            services
                .get(Value::from("backend"))
                .unwrap()
                .get("port")
                .and_then(|p| p.as_u64()),
            Some(8081)
        );
    }

    #[test]
    fn combo_port_conflict_and_unknown_block_rejected() {
        let mut ports = BTreeMap::new();
        ports.insert("backend".to_string(), 8081u32);
        ports.insert("web".to_string(), 8081u32);
        let err = preview_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            None,
            None,
            &ports,
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateBlockPort);

        let err = preview_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            None,
            Some(&["nope".to_string()]),
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::TemplateBlockDep);
    }

    #[test]
    fn combo_create_writes_selected_files_and_generated_yaml() {
        let parent = temp_dir("combo-create");
        let mut ports = BTreeMap::new();
        ports.insert("backend".to_string(), 9000u32);
        ports.insert("web".to_string(), 6000u32);
        let target = create_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            &parent,
            "combo-app",
            None,
            &BTreeMap::new(),
            Some(&["backend".to_string(), "web".to_string()]),
            &ports,
        )
        .unwrap();

        for rel in [
            "pom.xml",
            "backend/pom.xml",
            "web/server.js",
            "supertask.yaml",
        ] {
            assert!(
                target
                    .join(rel.replace('/', std::path::MAIN_SEPARATOR_STR))
                    .is_file(),
                "缺少 {rel}"
            );
        }
        let text = fs::read_to_string(target.join("supertask.yaml")).unwrap();
        let (file, warnings) = parse_yaml(&text).unwrap();
        assert!(warnings.is_empty());
        assert_eq!(file.services.len(), 2);
        let backend = file.services.get("backend").unwrap();
        assert_eq!(backend.port, Some(9000));
        let health = backend.health.as_ref().unwrap();
        assert_eq!(
            health.http.as_deref(),
            Some("http://127.0.0.1:9000/actuator/health"),
            "{{port}} 占位应跟随端口分配"
        );
        assert_eq!(
            file.services.get("web").unwrap().depends_on,
            vec!["backend"]
        );
        let tpl = file.templates.as_ref().unwrap();
        assert_eq!(
            tpl.as_mapping()
                .unwrap()
                .get(Value::from("id"))
                .and_then(|v| v.as_str()),
            Some("spring-node-combo")
        );

        // 非空目标拒绝仍适用
        let err = create_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            &parent,
            "combo-app",
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
    fn combo_create_with_default_selection_matches_preview() {
        // blocks=None = 全块：产物与 preview 的 services 一致（预览无副作用的契约）
        let out = preview_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            None,
            None,
            &BTreeMap::new(),
            &BTreeMap::new(),
        )
        .unwrap();
        let parent = temp_dir("combo-default");
        let target = create_template(
            "spring-node-combo",
            TemplateSourceKind::Builtin,
            &parent,
            "combo-default",
            None,
            &BTreeMap::new(),
            None,
            &BTreeMap::new(),
        )
        .unwrap();
        let (file, _) =
            parse_yaml(&fs::read_to_string(target.join("supertask.yaml")).unwrap()).unwrap();
        assert_eq!(
            file.services.len(),
            out.services.as_mapping().unwrap().len()
        );
        let _ = fs::remove_dir_all(&parent);
    }
}
