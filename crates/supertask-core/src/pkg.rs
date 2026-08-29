//! 工作区导出包（1.5 §6）：zip = manifest.json + supertask.yaml + 可选密钥文件。
//!
//! 包格式 format:1 字段只增不破（2.0 一键迁移载荷雏形）。导出默认不含密钥；
//! import 只落盘零执行：逐条 sha256 校验 + 路径安全检查（zip-slip）。
//!
//! 实现偏差（已在实现计划复用核查记档）：zip 依赖落在 core——桌面
//! exportPackage/importPackage 与 CLI 共用同一实现，字节流接口拆两份写入器
//! 反而让壳层各带一份格式代码。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, ErrorCode, Result};
use crate::spec::parse_yaml;

pub const PKG_FORMAT: u32 = 1;
pub const MANIFEST_NAME: &str = "manifest.json";
pub const SPEC_NAME: &str = "supertask.yaml";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkgEntry {
    /// zip 内路径，一律 `/` 分隔、UTF-8
    pub path: String,
    /// 内容 sha256（hex 小写）
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PkgManifest {
    pub format: u32,
    pub name: String,
    /// epoch 毫秒
    pub created_at: u64,
    pub source_os: String,
    pub app_version: String,
    /// 内容清单（不含 manifest.json 本身）
    pub entries: Vec<PkgEntry>,
}

#[derive(Debug, Clone)]
pub struct ExportOutcome {
    pub path: PathBuf,
    /// 内容条目（与 manifest.entries 一致；IPC 输出取 path/bytes）
    pub entries: Vec<PkgEntry>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ImportOutcome {
    pub root: PathBuf,
    pub warnings: Vec<String>,
}

/// 导出工作区包。`with_secrets=false` 时 secrets.file 与全部 env_file 一律排除。
pub fn export_package(root: &Path, dest: &Path, with_secrets: bool) -> Result<ExportOutcome> {
    let yaml_path = root.join(SPEC_NAME);
    let yaml_bytes = fs::read(&yaml_path)
        .map_err(|e| Error::new(ErrorCode::NoYaml, format!("无法读取 {SPEC_NAME}: {e}")))?;
    let (spec, _) = parse_yaml(&String::from_utf8_lossy(&yaml_bytes))?;

    // ---- 密钥文件收集（去重 + 沙箱校验）----
    let mut secret_rels: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    if let Some(secrets) = &spec.secrets {
        if let Some(f) = &secrets.file {
            secret_rels.push(f.clone());
        }
        if with_secrets
            && matches!(secrets.backend, Some(crate::spec::SecretsBackend::Env))
            && secrets.file.is_none()
        {
            warnings.push("secrets.backend: env 无密钥文件可打包".into());
        }
    }
    for svc in spec.services.values() {
        for f in &svc.env_file {
            secret_rels.push(f.clone());
        }
    }
    secret_rels.dedup_by(|a, b| normalize_rel(a) == normalize_rel(b));

    let mut content: Vec<(String, Vec<u8>)> = vec![(SPEC_NAME.to_string(), yaml_bytes)];
    if with_secrets {
        for rel in &secret_rels {
            let abs = root.join(rel);
            match safe_workspace_file(root, &abs) {
                Err(msg) => warnings.push(format!("密钥文件跳过: {rel}（{msg}）")),
                Ok(None) => warnings.push(format!("密钥文件跳过: {rel}（文件不存在）")),
                Ok(Some(bytes)) => content.push((normalize_rel(rel), bytes)),
            }
        }
    }

    // ---- manifest ----
    let entries: Vec<PkgEntry> = content
        .iter()
        .map(|(path, bytes)| PkgEntry {
            path: path.clone(),
            sha256: sha256_hex(bytes),
            bytes: bytes.len() as u64,
        })
        .collect();
    let manifest = PkgManifest {
        format: PKG_FORMAT,
        name: spec
            .name
            .clone()
            .unwrap_or_else(|| root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()),
        created_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
        source_os: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        entries,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest)
        .map_err(|e| Error::new(ErrorCode::PkgInvalid, format!("manifest 序列化失败: {e}")))?;

    // ---- zip 写出（Deflate）----
    let mut writer = zip::ZipWriter::new(
        fs::File::create(dest).map_err(|e| Error::new(ErrorCode::PkgInvalid, format!("无法创建输出文件: {e}")))?,
    );
    let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (path, bytes) in content.iter().chain(std::iter::once(&(MANIFEST_NAME.to_string(), manifest_bytes))) {
        writer
            .start_file(path.as_str(), opts)
            .map_err(|e| pkg_err("写入条目失败", path, e))?;
        use std::io::Write as _;
        writer
            .write_all(bytes)
            .map_err(|e| pkg_err("写入内容失败", path, e))?;
    }
    writer.finish().map_err(|e| {
        Error::new(ErrorCode::PkgInvalid, format!("zip 收尾失败: {e}"))
    })?;

    Ok(ExportOutcome { path: dest.to_path_buf(), entries: manifest.entries, warnings })
}

/// 导入工作区包（只落盘，零执行；dest 已有 supertask.yaml → PKG_TARGET_EXISTS）。
pub fn import_package(pkg: &Path, dest: &Path) -> Result<ImportOutcome> {
    if !pkg.is_file() {
        return Err(Error::new(
            ErrorCode::PkgNotFound,
            format!("导出包不存在: {}", pkg.display()),
        ));
    }
    let mut warnings: Vec<String> = Vec::new();
    let mut archive = zip::ZipArchive::new(
        fs::File::open(pkg).map_err(|e| Error::new(ErrorCode::PkgNotFound, format!("导出包不可读: {e}")))?,
    )
    .map_err(|e| Error::new(ErrorCode::PkgInvalid, format!("zip 解析失败: {e}")))?;

    // ---- manifest ----
    let manifest_bytes = read_entry(&mut archive, MANIFEST_NAME)?;
    let manifest: PkgManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| Error::new(ErrorCode::PkgInvalid, format!("manifest 损坏: {e}")))?;
    if manifest.format > PKG_FORMAT {
        return Err(Error::new(
            ErrorCode::PkgVersion,
            format!(
                "包 format {} 高于支持版本 {PKG_FORMAT}，请升级 SuperTask",
                manifest.format
            ),
        ));
    }

    // ---- 逐条校验（哈希 + 路径安全）后落盘 ----
    if dest.join(SPEC_NAME).exists() {
        return Err(Error::new(
            ErrorCode::PkgTargetExists,
            format!("目标目录已有 {SPEC_NAME}，不覆盖: {}", dest.display()),
        ));
    }
    fs::create_dir_all(dest).map_err(|e| {
        Error::new(ErrorCode::PkgInvalid, format!("无法创建目标目录: {e}"))
    })?;
    let seen_in_manifest = manifest.entries.iter().map(|e| e.path.clone()).collect::<Vec<_>>();
    for entry in &manifest.entries {
        let rel = safe_rel_path(&entry.path).ok_or_else(|| {
            Error::new(ErrorCode::PkgInvalid, format!("路径不安全: {}", entry.path))
        })?;
        let bytes = read_entry(&mut archive, &entry.path)?;
        if bytes.len() as u64 != entry.bytes || sha256_hex(&bytes) != entry.sha256 {
            return Err(Error::new(
                ErrorCode::PkgInvalid,
                format!("条目哈希不符: {}", entry.path),
            ));
        }
        let abs = dest.join(&rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Error::new(ErrorCode::PkgInvalid, format!("无法创建目录: {e}"))
            })?;
        }
        fs::write(&abs, &bytes).map_err(|e| {
            Error::new(ErrorCode::PkgInvalid, format!("写入失败 {}: {e}", entry.path))
        })?;
    }

    // ---- 包内 manifest 之外的条目：警告 + 跳过（不落盘）----
    for i in 0..archive.len() {
        let name = archive
            .by_index_raw(i)
            .map_err(|e| pkg_err("读取条目失败", "", e))?
            .name()
            .to_string();
        if name == MANIFEST_NAME || seen_in_manifest.contains(&name) || name.ends_with('/') {
            continue;
        }
        warnings.push(format!("包内多余条目已跳过: {name}"));
    }

    Ok(ImportOutcome { root: dest.to_path_buf(), warnings })
}

/// 条目路径安全检查：拒绝绝对路径、`..`、反斜杠、盘符（zip-slip 防线之一，
/// 另一条防线是「我们从不创建符号链接条目」——落盘全部为普通文件）。
fn safe_rel_path(p: &str) -> Option<PathBuf> {
    if p.is_empty() || p.contains('\\') || p.contains(':') || p.starts_with('/') {
        return None;
    }
    let mut out = PathBuf::new();
    for comp in p.split('/') {
        match comp {
            "" | "." => {}
            ".." => return None,
            c => out.push(c),
        }
    }
    if out.as_os_str().is_empty() { None } else { Some(out) }
}

/// 密钥文件读取：必须在 root 内（canonicalize 后前缀校验）且存在。
fn safe_workspace_file(root: &Path, abs: &Path) -> std::result::Result<Option<Vec<u8>>, String> {
    if !abs.is_file() {
        return Ok(None);
    }
    let canon_root = root.canonicalize().map_err(|e| e.to_string())?;
    let canon = abs.canonicalize().map_err(|e| e.to_string())?;
    if !canon.starts_with(&canon_root) {
        return Err("路径逃逸出工作区".into());
    }
    fs::read(&canon).map(Some).map_err(|e| e.to_string())
}

fn read_entry(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut f = archive
        .by_name(name)
        .map_err(|_| Error::new(ErrorCode::PkgInvalid, format!("包内缺少条目: {name}")))?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut out)
        .map_err(|e| Error::new(ErrorCode::PkgInvalid, format!("读取条目失败 {name}: {e}")))?;
    Ok(out)
}

/// 条目级错误包装（zip::ZipError 与 io::Error 通用）。
fn pkg_err(msg: &str, path: &str, e: impl std::fmt::Display) -> Error {
    Error::new(ErrorCode::PkgInvalid, format!("{msg} {path}: {e}"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

/// 归一化相对路径分隔符（去重比较用；Windows 用户常写 `\`）。
fn normalize_rel(p: &str) -> String {
    p.replace('\\', "/").trim_start_matches("./").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-pkg-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    const YAML: &str = "version: 1\nname: demo\nservices:\n  api:\n    kind: spring-boot\n    module: m\n    port: 8080\n    env_file:\n      - .env.shared\nsecrets:\n  backend: file\n  file: .env.local\n";

    fn seed_workspace(root: &Path) {
        fs::write(root.join(SPEC_NAME), YAML).unwrap();
        fs::write(root.join(".env.local"), "DB_PASSWORD=hunter2\n").unwrap();
        fs::write(root.join(".env.shared"), "LOG_LEVEL=info\n").unwrap();
        fs::create_dir_all(root.join(".supertask")).unwrap();
        fs::write(root.join(".supertask/engine.lock"), "junk").unwrap();
    }

    #[test]
    fn export_default_excludes_secrets_and_runtime_files() {
        let root = temp_root("export-default");
        seed_workspace(&root);
        let dest = root.parent().unwrap().join("pkg-default.zip");
        let out = export_package(&root, &dest, false).unwrap();
        assert_eq!(out.entries.len(), 1);
        assert_eq!(out.entries[0].path, SPEC_NAME);
        assert!(out.path.is_file());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&dest);
    }

    #[test]
    fn export_with_secrets_includes_and_dedups() {
        let root = temp_root("export-secrets");
        seed_workspace(&root);
        let dest = root.parent().unwrap().join("pkg-secrets.zip");
        let out = export_package(&root, &dest, true).unwrap();
        let paths: Vec<&str> = out.entries.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(paths, vec![SPEC_NAME, ".env.local", ".env.shared"]);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&dest);
    }

    #[test]
    fn import_round_trip_yaml_bytes_equal() {
        let root = temp_root("roundtrip");
        seed_workspace(&root);
        let dest_zip = root.parent().unwrap().join("pkg-rt.zip");
        export_package(&root, &dest_zip, true).unwrap();
        let dest_dir = root.parent().unwrap().join("imported-ws");
        let _ = fs::remove_dir_all(&dest_dir);
        let out = import_package(&dest_zip, &dest_dir).unwrap();
        assert_eq!(out.root, dest_dir);
        assert_eq!(
            fs::read(dest_dir.join(SPEC_NAME)).unwrap(),
            YAML.as_bytes()
        );
        assert_eq!(
            fs::read_to_string(dest_dir.join(".env.local")).unwrap(),
            "DB_PASSWORD=hunter2\n"
        );
        // 不搬运运行时产物
        assert!(!dest_dir.join(".supertask").exists());
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&dest_zip);
        let _ = fs::remove_dir_all(&dest_dir);
    }

    #[test]
    fn import_rejects_target_exists() {
        let root = temp_root("target-exists");
        seed_workspace(&root);
        let dest_zip = root.parent().unwrap().join("pkg-te.zip");
        export_package(&root, &dest_zip, false).unwrap();
        let err = import_package(&dest_zip, &root).unwrap_err();
        assert_eq!(err.code(), ErrorCode::PkgTargetExists);
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&dest_zip);
    }

    #[test]
    fn import_rejects_missing_and_corrupt_pkg() {
        let missing = std::env::temp_dir().join(format!("st-pkg-missing-{}.zip", std::process::id()));
        let dest = temp_root("missing-dest");
        assert_eq!(import_package(&missing, &dest).unwrap_err().code(), ErrorCode::PkgNotFound);

        let corrupt = std::env::temp_dir().join(format!("st-pkg-corrupt-{}.zip", std::process::id()));
        fs::write(&corrupt, b"not a zip").unwrap();
        assert_eq!(import_package(&corrupt, &dest).unwrap_err().code(), ErrorCode::PkgInvalid);
        let _ = fs::remove_dir_all(&dest);
        let _ = fs::remove_file(&corrupt);
    }

    /// 测试助手：手工打包任意条目（构造恶意包用）。
    fn write_test_zip(dest: &Path, entries: &[(&str, Vec<u8>)]) {
        let file = fs::File::create(dest).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> = zip::write::FileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            w.start_file(*name, opts).unwrap();
            use std::io::Write as _;
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    fn manifest_json(format: u32, entries: Vec<PkgEntry>) -> Vec<u8> {
        serde_json::to_vec(&PkgManifest {
            format,
            name: "t".into(),
            created_at: 0,
            source_os: "windows".into(),
            app_version: "0.0.0".into(),
            entries,
        })
        .unwrap()
    }

    #[test]
    fn import_rejects_zip_slip() {
        let root = temp_root("zipslip");
        let dest_zip = root.join("slip.zip");
        let dest_dir = root.join("out");
        let yaml = b"version: 1\nservices: {}\n".to_vec();
        let entries = vec![PkgEntry {
            path: "../escape.yaml".into(),
            sha256: sha256_hex(&yaml),
            bytes: yaml.len() as u64,
        }];
        write_test_zip(
            &dest_zip,
            &[
                (MANIFEST_NAME, manifest_json(PKG_FORMAT, entries)),
                (SPEC_NAME, yaml.clone()),
                ("../escape.yaml", yaml),
            ],
        );
        assert_eq!(import_package(&dest_zip, &dest_dir).unwrap_err().code(), ErrorCode::PkgInvalid);
        assert!(!root.join("escape.yaml").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn import_rejects_bad_hash_and_missing_entry() {
        let root = temp_root("badhash");
        let dest_zip = root.join("bad.zip");
        let dest_dir = root.join("out");
        let yaml = b"version: 1\nservices: {}\n".to_vec();

        // 哈希不符
        let entries = vec![PkgEntry {
            path: SPEC_NAME.into(),
            sha256: "00".repeat(32),
            bytes: yaml.len() as u64,
        }];
        write_test_zip(
            &dest_zip,
            &[(MANIFEST_NAME, manifest_json(PKG_FORMAT, entries)), (SPEC_NAME, yaml.clone())],
        );
        assert_eq!(import_package(&dest_zip, &dest_dir).unwrap_err().code(), ErrorCode::PkgInvalid);

        // manifest 声明了 zip 里不存在的条目
        let entries = vec![
            PkgEntry { path: SPEC_NAME.into(), sha256: sha256_hex(&yaml), bytes: yaml.len() as u64 },
            PkgEntry { path: ".env.local".into(), sha256: sha256_hex(b"x"), bytes: 1 },
        ];
        write_test_zip(
            &dest_zip,
            &[(MANIFEST_NAME, manifest_json(PKG_FORMAT, entries)), (SPEC_NAME, yaml)],
        );
        assert_eq!(import_package(&dest_zip, &dest_dir).unwrap_err().code(), ErrorCode::PkgInvalid);

        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn import_rejects_future_format() {
        let root = temp_root("future-format");
        let dest_zip = root.join("future.zip");
        let yaml = b"version: 1\nservices: {}\n".to_vec();
        let entries = vec![PkgEntry {
            path: SPEC_NAME.into(),
            sha256: sha256_hex(&yaml),
            bytes: yaml.len() as u64,
        }];
        write_test_zip(
            &dest_zip,
            &[(MANIFEST_NAME, manifest_json(PKG_FORMAT + 1, entries)), (SPEC_NAME, yaml)],
        );
        let dest_dir = root.join("out");
        assert_eq!(import_package(&dest_zip, &dest_dir).unwrap_err().code(), ErrorCode::PkgVersion);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn safe_rel_path_accepts_plain() {
        assert_eq!(safe_rel_path("supertask.yaml").as_deref(), Some(Path::new("supertask.yaml")));
        assert_eq!(safe_rel_path(".env.local").as_deref(), Some(Path::new(".env.local")));
    }
}
