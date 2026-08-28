//! 1.1 内置官方模板：编译期嵌入资源 + manifest 摘要校验 + 创建工作区。
//!
//! 规格见 `docs/plans/2026-08-27-v1-1-feature-spec.md` §4：
//! - 资源用 `include_dir` 编译期嵌入（`crates/supertask-core/template_assets/`），
//!   离线分发，不依赖网络，也不信任运行时传入的模板源；
//! - manifest 硬编码在本文件，逐文件 sha256 校验，防止改模板不改清单；
//! - `create_template` 先做目录安全与目标检查，复制前校验资源，失败不落盘；
//!   写失败不删除已复制文件，也不显示成功。

use std::fs;
use std::path::{Path, PathBuf};

use include_dir::{include_dir, Dir};
use serde::Serialize;
use serde_yaml::Value;
use sha2::{Digest, Sha256};

use crate::error::{Error, ErrorCode, Result};
use crate::sandbox::strip_verbatim;
use crate::spec::{parse_yaml, SuperTaskFile};

/// 编译期嵌入 `crates/supertask-core/template_assets` 下全部模板资源。
static TEMPLATE_ASSETS: Dir = include_dir!("$CARGO_MANIFEST_DIR/template_assets");

/// 嵌入资源里忽略的构建产物/版本库目录（模板目录被误构建时不污染嵌入内容）。
const SKIP_DIRS: &[&str] = &["target", "node_modules", "dist"];

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
}

/// 内置模板 manifest：概览 + 每文件 sha256（十六进制小写，与嵌入资源逐一比对）。
struct BuiltinTemplate {
    summary: TemplateSummary,
    /// (相对路径, sha256)，路径按升序排列。
    sha256: Vec<(&'static str, &'static str)>,
}

impl BuiltinTemplate {
    fn new(
        id: &'static str,
        name: &'static str,
        description: &'static str,
        stacks: &[&str],
        files: &[(&'static str, &'static str)],
    ) -> Self {
        Self {
            summary: TemplateSummary {
                id: id.into(),
                version: "1".into(),
                name: name.into(),
                description: description.into(),
                stacks: stacks.iter().map(|s| s.to_string()).collect(),
                files: files.iter().map(|(f, _)| f.to_string()).collect(),
            },
            sha256: files.to_vec(),
        }
    }
}

/// 内置模板 manifest（硬编码真源；sha256 与嵌入资源不一致会在校验期报
/// `TEMPLATE_INVALID`，单元测试也会比对失败）。
fn builtin_manifests() -> Vec<BuiltinTemplate> {
    vec![
        BuiltinTemplate::new(
            "spring-multimodule-node",
            "Spring 多模块 + Node（完整示例）",
            "Spring Boot 多模块后端 + 零依赖 Node 前端，含健康检查与依赖关系",
            &["spring-boot", "node"],
            &[
                (
                    "backend/pom.xml",
                    "60b7569cf6b31191431b8001697b49cafa6195a3a97f201f0e0fd8f8b6d86483",
                ),
                (
                    "backend/src/main/java/com/supertask/demo/DemoApplication.java",
                    "2d452d43cdcb0b7f5fa9f9fb3a96de2800426f6ef6224ad1eb51e850a47f07ce",
                ),
                (
                    "backend/src/main/resources/application.properties",
                    "19b839d80bf7ce5009059a7c247c60ae0df99eaf26c4d7d78e0db7b0c5a330f5",
                ),
                (
                    "pom.xml",
                    "5af84a2a0d0f4e84b0d4f69927c94899c361b8160c681858cc84f1f042a5881f",
                ),
                (
                    "supertask.yaml",
                    "019a6c8866cb1efd44025ad0c01e1391113774b9d634962b7b24e7099c66bc5a",
                ),
                (
                    "web/package.json",
                    "41a044532183fd820219107ef305a98f3f5a22f55cedffa2672e5f8860938a7f",
                ),
                (
                    "web/server.js",
                    "b26dc2ed088978a83ccdf7b441ff0c0e7033e32d09b17aab63c07ff7decfb3f4",
                ),
            ],
        ),
        BuiltinTemplate::new(
            "spring-multimodule-node-minimal",
            "Spring 多模块 + Node（最小起步）",
            "一个可运行的 Spring 模块 + 一个 Node 服务，YAML 精简，健康检查由引擎兜底",
            &["spring-boot", "node"],
            &[
                (
                    "backend/pom.xml",
                    "54e1a497f8052336358ce6cc45e542a321bcc540cc9b0530de27f2e6814e1dbc",
                ),
                (
                    "backend/src/main/java/com/supertask/demo/DemoApplication.java",
                    "2652608c3c6628fa39f947a0dd542607c8ba51f2a490759ac5cea89d14a1ef89",
                ),
                (
                    "pom.xml",
                    "13b293e6bab207b9c088c6eadaa2f363589a2e5abd144647ebd33ece5d07ffef",
                ),
                (
                    "supertask.yaml",
                    "3aea31f9aa47527af16c0342d76601ee784790784a44234b301d56ba6378f178",
                ),
                (
                    "web/package.json",
                    "783aa896d1e42bf8c246ebca087cb7e096a28c9d3b19fd384bdc4b5f063b284e",
                ),
                (
                    "web/server.js",
                    "adcd4fb6fe80c853f31a762f26ede0a83a92f8d2298f6dc384b82ae52876bd86",
                ),
            ],
        ),
    ]
}

/// 枚举内置官方模板（IPC `templates.list`）。
pub fn list_templates() -> Vec<TemplateSummary> {
    builtin_manifests().into_iter().map(|m| m.summary).collect()
}

/// 用内置模板在 `parent_path/directory_name` 创建新工作区，返回工作区根目录。
pub fn create_template(
    template_id: &str,
    parent_path: &Path,
    directory_name: &str,
) -> Result<PathBuf> {
    let manifests = builtin_manifests();
    let manifest = manifests
        .iter()
        .find(|m| m.summary.id == template_id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("模板不存在: {template_id}")))?;

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

    // 复制前先校验嵌入资源与 manifest 一致，失败不落盘
    let files = verify_assets(manifest)?;

    // 逐文件复制；supertask.yaml 注入 templates 保留段后最后写
    for (rel, bytes) in &files {
        if rel == "supertask.yaml" {
            continue;
        }
        write_asset_file(&target, rel, bytes)?;
    }

    let template_yaml = files
        .iter()
        .find(|(rel, _)| rel == "supertask.yaml")
        .map(|(_, bytes)| *bytes)
        .ok_or_else(|| {
            Error::new(ErrorCode::TemplateInvalid, "模板缺少 supertask.yaml")
        })?;
    let yaml_text = build_workspace_yaml(template_yaml, manifest)?;
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

    // 规格要求：创建完成后对关键文件做存在性校验
    for rel in &manifest.summary.files {
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
        "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7",
        "COM8", "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
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

/// 校验嵌入资源与 manifest 的文件集合及逐文件 sha256 完全一致；
/// 通过后返回 (相对路径, 内容) 列表（按路径升序）。
fn verify_assets(manifest: &BuiltinTemplate) -> Result<Vec<(String, &'static [u8])>> {
    let template_dir = TEMPLATE_ASSETS
        .get_dir(manifest.summary.id.as_str())
        .ok_or_else(|| {
            Error::new(
                ErrorCode::TemplateInvalid,
                format!("嵌入资源缺少模板目录 {}", manifest.summary.id),
            )
        })?;

    let mut actual: Vec<(String, &'static [u8])> = Vec::new();
    collect_embedded_files(template_dir, &mut actual);
    // 兼容 include_dir 的路径语义差异：条目路径可能带模板 id 前缀
    let prefix = format!("{}/", manifest.summary.id);
    for (path, _) in actual.iter_mut() {
        if let Some(stripped) = path.strip_prefix(&prefix) {
            *path = stripped.to_string();
        }
    }
    actual.sort_by(|a, b| a.0.cmp(&b.0));

    let mut expected: Vec<&str> = manifest.sha256.iter().map(|(p, _)| *p).collect();
    expected.sort_unstable();
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
                manifest.summary.id
            ),
        ));
    }

    for (rel, bytes) in &actual {
        let expected_hash = manifest
            .sha256
            .iter()
            .find(|(p, _)| p == rel)
            .map(|(_, h)| *h)
            .unwrap_or_default();
        let actual_hash = sha256_hex(bytes);
        if actual_hash != expected_hash {
            return Err(Error::new(
                ErrorCode::TemplateInvalid,
                format!("模板 {} 文件摘要不匹配: {rel}", manifest.summary.id),
            ));
        }
    }
    Ok(actual)
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

/// 复制单个模板文件到目标目录；失败返回 `TEMPLATE_WRITE`，message 含失败路径。
fn write_asset_file(target: &Path, rel: &str, bytes: &[u8]) -> Result<()> {
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

/// 取模板自带 supertask.yaml，注入 `templates` 保留段后重新序列化。
fn build_workspace_yaml(template_yaml: &[u8], manifest: &BuiltinTemplate) -> Result<String> {
    let text = std::str::from_utf8(template_yaml)
        .map_err(|_| Error::new(ErrorCode::TemplateInvalid, "模板 supertask.yaml 不是 UTF-8"))?;
    let mut file: SuperTaskFile = serde_yaml::from_str(text).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板 supertask.yaml 解析失败: {e}"),
        )
    })?;
    let mut section = serde_yaml::Mapping::new();
    section.insert(Value::from("source"), Value::from("builtin"));
    section.insert(Value::from("id"), Value::from(manifest.summary.id.as_str()));
    section.insert(
        Value::from("version"),
        Value::from(manifest.summary.version.as_str()),
    );
    file.templates = Some(Value::Mapping(section));
    serde_yaml::to_string(&file).map_err(|e| {
        Error::new(
            ErrorCode::TemplateInvalid,
            format!("模板 supertask.yaml 序列化失败: {e}"),
        )
    })
}

fn sha256_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;

    fn temp_parent(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-tpl-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn manifest_matches_embedded_assets() {
        let manifests = builtin_manifests();
        assert!(manifests.len() >= 2);
        for manifest in &manifests {
            let actual = verify_assets(manifest)
                .unwrap_or_else(|e| panic!("模板 {} 校验失败: {e}", manifest.summary.id));
            // files 概览与嵌入实际文件集合完全一致（双向）
            let mut expected: Vec<&str> =
                manifest.summary.files.iter().map(|s| s.as_str()).collect();
            expected.sort_unstable();
            let mut actual_paths: Vec<&str> =
                actual.iter().map(|(p, _)| p.as_str()).collect();
            actual_paths.sort_unstable();
            assert_eq!(expected, actual_paths, "模板 {} files 概览不一致", manifest.summary.id);
        }
        // 嵌入根下不允许出现 manifest 之外的散落模板目录
        for dir in TEMPLATE_ASSETS.dirs() {
            let name = dir.path().file_name().unwrap().to_string_lossy().into_owned();
            assert!(
                manifests.iter().any(|m| m.summary.id == name),
                "嵌入目录 {name} 未登记在 manifest 中"
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
            let err = create_template("spring-multimodule-node", &parent, name).unwrap_err();
            assert_eq!(err.code(), ErrorCode::PathEscape, "目录名 {name:?} 应被拒绝");
        }
        for ok in ["demo-app", "my_workspace", "项目01", "a.b.c"] {
            validate_directory_name(ok).unwrap_or_else(|e| panic!("目录名 {ok:?} 应合法: {e}"));
        }
    }

    #[test]
    fn unknown_template_id_not_found() {
        let parent = temp_parent("unknown");
        let err = create_template("no-such-template", &parent, "demo").unwrap_err();
        assert_eq!(err.code(), ErrorCode::NotFound);
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn missing_parent_is_cwd_missing() {
        let missing = std::env::temp_dir()
            .join(format!("st-tpl-missing-{}", std::process::id()))
            .join("no-such-parent");
        let err =
            create_template("spring-multimodule-node", &missing, "demo").unwrap_err();
        assert_eq!(err.code(), ErrorCode::CwdMissing);
    }

    #[test]
    fn creates_new_directory_and_yaml_has_templates_section() {
        let parent = temp_parent("create");
        let target = create_template("spring-multimodule-node", &parent, "demo-app").unwrap();
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
            assert!(target.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR)).is_file(), "缺少 {rel}");
        }

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
        let parent = temp_parent("empty");
        fs::create_dir_all(parent.join("dst")).unwrap();
        let target = create_template("spring-multimodule-node", &parent, "dst").unwrap();
        assert!(target.join("pom.xml").is_file());
        let _ = fs::remove_dir_all(&parent);
    }

    #[test]
    fn non_empty_target_rejected_and_untouched() {
        let parent = temp_parent("nonempty");
        let dst = parent.join("dst");
        fs::create_dir_all(dst.join("keep-dir")).unwrap();
        fs::write(dst.join("keep.txt"), "原样内容").unwrap();
        fs::write(dst.join("keep-dir/nested.txt"), "嵌套").unwrap();

        let err =
            create_template("spring-multimodule-node", &parent, "dst").unwrap_err();
        assert_eq!(err.code(), ErrorCode::TargetNotEmpty);

        // 原目录内容一字未动，也没有混入模板文件
        assert_eq!(fs::read_to_string(dst.join("keep.txt")).unwrap(), "原样内容");
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
        let manifests = builtin_manifests();
        let minimal = manifests
            .iter()
            .find(|m| m.summary.id == "spring-multimodule-node-minimal")
            .unwrap();
        let files = verify_assets(minimal).unwrap();
        let (_, yaml_bytes) = files.iter().find(|(p, _)| p == "supertask.yaml").unwrap();
        let text = std::str::from_utf8(yaml_bytes).unwrap();
        // 结构化断言：模板自带 yaml 的 backend 不含 health 键（注释里的文字不算）
        let raw: Value = serde_yaml::from_str(text).unwrap();
        assert!(
            raw.get("services").and_then(|s| s.get("backend")).unwrap().get("health").is_none(),
            "最小模板 backend 不应自带 health 字段"
        );

        let mut file: SuperTaskFile = serde_yaml::from_str(text).unwrap();
        assert!(file.services.get("backend").unwrap().health.is_none());
        file.apply_defaults();
        assert!(file.services.get("backend").unwrap().health.is_some());
    }
}
