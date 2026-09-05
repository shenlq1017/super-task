//! 方向六·数据与备份：服务绑定数据卷的离线快照（ipc.md §10.18）。
//!
//! zip = `manifest.json` + `data/<相对路径>`；逐条 sha256（口径同 pkg 导出包）。
//! 恢复 = 目录内容替换：整包校验 → 现目录 stash → 解压 → 失败回滚。
//! 快照是**离线**文件快照：绑定服务未停止由 engine 层拒绝（SNAPSHOT_BUSY），
//! 本模块不感知运行时状态。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::error::{Error, ErrorCode, Result};

pub const SNAPSHOT_FORMAT: u32 = 1;
pub const MANIFEST_NAME: &str = "manifest.json";
/// zip 内数据条目前缀（与 manifest.json 区隔）。
pub const DATA_PREFIX: &str = "data/";
/// 单快照条目上限。
pub const MAX_SNAPSHOT_ENTRIES: usize = 20_000;
/// 单快照解压总字节上限（512 MiB）。
pub const MAX_SNAPSHOT_TOTAL_BYTES: u64 = 512 * 1024 * 1024;
/// 恢复预览 remove_sample 上限。
pub const PREVIEW_SAMPLE_LIMIT: usize = 20;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotEntry {
    /// 卷内相对路径，一律 `/` 分隔、UTF-8
    pub path: String,
    /// 内容 sha256（hex 小写）
    pub sha256: String,
    pub bytes: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotManifest {
    pub format: u32,
    pub volume_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub service: Option<String>,
    #[serde(default)]
    pub note: String,
    /// epoch 毫秒
    pub created_at: u64,
    pub source_os: String,
    pub app_version: String,
    pub file_count: u64,
    pub total_bytes: u64,
    pub entries: Vec<SnapshotEntry>,
}

/// 快照元信息（IPC `DataSnapshotView` 的来源）。
#[derive(Debug, Clone)]
pub struct SnapshotMeta {
    /// created_at 毫秒字符串（文件名 stem）
    pub id: String,
    pub created_at: u64,
    /// zip 文件大小
    pub bytes: u64,
    pub file_count: u64,
    pub total_bytes: u64,
    pub note: String,
}

/// 恢复预览（纯只读，restore 前的覆盖面陈述）。
#[derive(Debug, Clone)]
pub struct RestorePreview {
    pub target_exists: bool,
    pub current_files: u64,
    pub snapshot_files: u64,
    pub total_bytes: u64,
    /// 快照外现存文件数（恢复后被删除）
    pub remove_count: u64,
    /// 最多 [`PREVIEW_SAMPLE_LIMIT`] 条相对路径
    pub remove_sample: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct RestoreOutcome {
    pub restored_files: u64,
    pub removed_files: u64,
}

/// 快照（离线文件快照）。`dir` 不存在或不是目录即失败——宁可拒绝也不产出空快照。
pub fn create_snapshot(
    dir: &Path,
    out_dir: &Path,
    volume_id: &str,
    service: Option<&str>,
    note: &str,
) -> Result<SnapshotMeta> {
    if !dir.is_dir() {
        return Err(Error::new(
            ErrorCode::SnapshotInvalid,
            format!("数据目录不存在或不是目录: {}", dir.display()),
        ));
    }
    let mut files: Vec<(String, PathBuf)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    collect_files(dir, dir, &mut files, &mut warnings)?;

    let mut entries: Vec<SnapshotEntry> = Vec::with_capacity(files.len());
    let mut total_bytes: u64 = 0;
    for (rel, abs) in &files {
        let bytes = fs::read(abs).map_err(|e| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!(
                    "无法读取 {}: {e}（服务可能未停止或文件被占用）",
                    abs.display()
                ),
            )
        })?;
        total_bytes += bytes.len() as u64;
        entries.push(SnapshotEntry {
            path: format!("{DATA_PREFIX}{rel}"),
            sha256: sha256_hex(&bytes),
            bytes: bytes.len() as u64,
        });
    }
    check_caps(entries.len(), total_bytes)?;

    let created_at = now_ms();
    let manifest = SnapshotManifest {
        format: SNAPSHOT_FORMAT,
        volume_id: volume_id.to_string(),
        service: service.map(str::to_string),
        note: note.to_string(),
        created_at,
        source_os: std::env::consts::OS.to_string(),
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        file_count: entries.len() as u64,
        total_bytes,
        entries,
    };
    let manifest_bytes = serde_json::to_vec_pretty(&manifest).map_err(|e| {
        Error::new(
            ErrorCode::SnapshotInvalid,
            format!("manifest 序列化失败: {e}"),
        )
    })?;

    fs::create_dir_all(out_dir)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("无法创建快照目录: {e}")))?;
    let zip_path = out_dir.join(format!("{created_at}.zip"));
    write_zip(&zip_path, &files, &manifest_bytes)?;

    let bytes = fs::metadata(&zip_path)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("快照写入失败: {e}")))?
        .len();
    let _ = warnings;
    Ok(SnapshotMeta {
        id: created_at.to_string(),
        created_at,
        bytes,
        file_count: manifest.file_count,
        total_bytes: manifest.total_bytes,
        note: manifest.note,
    })
}

/// 列出目录下全部快照（created_at 降序）；损坏文件跳过并计入 warnings。
pub fn list_snapshots(out_dir: &Path) -> (Vec<SnapshotMeta>, Vec<String>) {
    let mut metas: Vec<SnapshotMeta> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let Ok(read) = fs::read_dir(out_dir) else {
        return (metas, warnings);
    };
    for e in read.flatten() {
        let path = e.path();
        if path.extension().and_then(|x| x.to_str()) != Some("zip") {
            continue;
        }
        match snapshot_meta_of(&path) {
            Ok(meta) => metas.push(meta),
            Err(_) => warnings.push(format!(
                "快照文件损坏已跳过: {}",
                path.file_name().unwrap_or_default().to_string_lossy()
            )),
        }
    }
    metas.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    (metas, warnings)
}

/// 读取 manifest（不校验条目哈希）。
pub fn read_manifest(zip_path: &Path) -> Result<SnapshotManifest> {
    let mut archive = open_zip(zip_path)?;
    let manifest_bytes = read_entry(&mut archive, MANIFEST_NAME)?;
    let manifest: SnapshotManifest = serde_json::from_slice(&manifest_bytes)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("manifest 损坏: {e}")))?;
    if manifest.format > SNAPSHOT_FORMAT {
        return Err(Error::new(
            ErrorCode::SnapshotVersion,
            format!(
                "快照 format {} 高于支持版本 {SNAPSHOT_FORMAT}，请升级 SuperTask",
                manifest.format
            ),
        ));
    }
    Ok(manifest)
}

/// 整包校验：manifest 可读 + 逐条（哈希、字节数、路径安全）核对。
pub fn verify_snapshot(zip_path: &Path) -> Result<SnapshotManifest> {
    let manifest = read_manifest(zip_path)?;
    let mut archive = open_zip(zip_path)?;
    for entry in &manifest.entries {
        let rel = safe_entry_rel(&entry.path).ok_or_else(|| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("条目路径不安全: {}", entry.path),
            )
        })?;
        let bytes = read_entry(&mut archive, &entry.path)?;
        if bytes.len() as u64 != entry.bytes || sha256_hex(&bytes) != entry.sha256 {
            return Err(Error::new(
                ErrorCode::SnapshotInvalid,
                format!("条目哈希不符: {}", entry.path),
            ));
        }
        let _ = rel;
    }
    Ok(manifest)
}

/// 恢复预览（纯只读）：整包校验 + 覆盖面统计。
pub fn restore_preview(zip_path: &Path, target_dir: &Path) -> Result<RestorePreview> {
    let manifest = verify_snapshot(zip_path)?;
    let snap_rels: std::collections::HashSet<String> = manifest
        .entries
        .iter()
        .filter_map(|e| safe_entry_rel(&e.path))
        .collect();
    let current = current_files(target_dir)?;
    let remove: Vec<&String> = current.iter().filter(|p| !snap_rels.contains(*p)).collect();
    let remove_sample = remove
        .iter()
        .take(PREVIEW_SAMPLE_LIMIT)
        .map(|p| (*p).clone())
        .collect();
    Ok(RestorePreview {
        target_exists: target_dir.is_dir(),
        current_files: current.len() as u64,
        snapshot_files: manifest.file_count,
        total_bytes: manifest.total_bytes,
        remove_count: remove.len() as u64,
        remove_sample,
    })
}

/// 恢复（目录内容替换）：校验 → stash → 解压 → 失败回滚。`stash_dir` 必须不存在。
pub fn restore_snapshot(
    zip_path: &Path,
    target_dir: &Path,
    stash_dir: &Path,
) -> Result<RestoreOutcome> {
    let manifest = verify_snapshot(zip_path)?;
    let current = current_files(target_dir)?;
    let snap_rels: std::collections::HashSet<String> = manifest
        .entries
        .iter()
        .filter_map(|e| safe_entry_rel(&e.path))
        .collect();
    let removed_files = current.iter().filter(|p| !snap_rels.contains(*p)).count() as u64;

    // stash：整体改名（同卷 rename 原子）；目标不存在则跳过。
    let stashed = target_dir.is_dir();
    if stashed {
        if stash_dir.exists() {
            return Err(Error::new(
                ErrorCode::SnapshotInvalid,
                format!("stash 目录已存在: {}", stash_dir.display()),
            ));
        }
        fs::rename(target_dir, stash_dir).map_err(|e| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("无法移走现有目录（可能被占用）: {e}"),
            )
        })?;
    }

    let extract_err = extract_all(zip_path, target_dir, &manifest);
    if let Err(e) = extract_err {
        // 回滚：删半成品，stash 还原。
        let _ = fs::remove_dir_all(target_dir);
        if stashed {
            let _ = fs::rename(stash_dir, target_dir);
        }
        return Err(e);
    }

    if stashed {
        let _ = fs::remove_dir_all(stash_dir);
    }
    Ok(RestoreOutcome {
        restored_files: manifest.file_count,
        removed_files,
    })
}

/// 删除快照文件。
pub fn delete_snapshot(zip_path: &Path) -> Result<()> {
    if !zip_path.is_file() {
        return Err(Error::new(
            ErrorCode::SnapshotNotFound,
            format!("快照不存在: {}", zip_path.display()),
        ));
    }
    fs::remove_file(zip_path)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("删除失败: {e}")))
}

/// 快照文件名 → 元信息（list 用；只读 manifest，不校验条目）。
fn snapshot_meta_of(zip_path: &Path) -> Result<SnapshotMeta> {
    let manifest = read_manifest(zip_path)?;
    let bytes = fs::metadata(zip_path)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("不可读: {e}")))?
        .len();
    Ok(SnapshotMeta {
        id: manifest.created_at.to_string(),
        created_at: manifest.created_at,
        bytes,
        file_count: manifest.file_count,
        total_bytes: manifest.total_bytes,
        note: manifest.note,
    })
}

fn check_caps(count: usize, total_bytes: u64) -> Result<()> {
    if count > MAX_SNAPSHOT_ENTRIES {
        return Err(Error::new(
            ErrorCode::SnapshotInvalid,
            format!("快照条目数 {count} 超上限 {MAX_SNAPSHOT_ENTRIES}"),
        ));
    }
    if total_bytes > MAX_SNAPSHOT_TOTAL_BYTES {
        return Err(Error::new(
            ErrorCode::SnapshotInvalid,
            format!("快照总字节 {total_bytes} 超上限 {MAX_SNAPSHOT_TOTAL_BYTES}"),
        ));
    }
    Ok(())
}

/// 递归收集普通文件（排序保证顺序稳定）；符号链接跳过并警告。
fn collect_files(
    root: &Path,
    dir: &Path,
    out: &mut Vec<(String, PathBuf)>,
    warnings: &mut Vec<String>,
) -> Result<()> {
    let read = fs::read_dir(dir).map_err(|e| {
        Error::new(
            ErrorCode::SnapshotInvalid,
            format!("无法读取目录 {}: {e}", dir.display()),
        )
    })?;
    let mut entries: Vec<_> = read.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let path = e.path();
        let Ok(ft) = e.file_type() else { continue };
        let rel = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if ft.is_symlink() {
            warnings.push(format!("符号链接已跳过: {rel}"));
            continue;
        }
        if ft.is_dir() {
            collect_files(root, &path, out, warnings)?;
        } else if ft.is_file() {
            out.push((rel, path));
        }
    }
    Ok(())
}

fn write_zip(zip_path: &Path, files: &[(String, PathBuf)], manifest_bytes: &[u8]) -> Result<()> {
    let tmp_path = zip_path.with_extension("zip.tmp");
    let mut writer =
        zip::ZipWriter::new(fs::File::create(&tmp_path).map_err(|e| {
            Error::new(ErrorCode::SnapshotInvalid, format!("无法创建临时文件: {e}"))
        })?);
    let opts: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let write_one = |w: &mut zip::ZipWriter<fs::File>, name: &str, bytes: &[u8]| -> Result<()> {
        w.start_file(name, opts).map_err(|e| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("写入条目失败 {name}: {e}"),
            )
        })?;
        use std::io::Write as _;
        w.write_all(bytes).map_err(|e| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("写入内容失败 {name}: {e}"),
            )
        })?;
        Ok(())
    };
    write_one(&mut writer, MANIFEST_NAME, manifest_bytes)?;
    for (rel, abs) in files {
        let bytes = fs::read(abs).map_err(|e| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("无法读取 {}: {e}", abs.display()),
            )
        })?;
        write_one(&mut writer, &format!("{DATA_PREFIX}{rel}"), &bytes)?;
    }
    writer
        .finish()
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("zip 收尾失败: {e}")))?;
    // 同毫秒重复创建时覆盖旧文件（Windows rename 不允许覆盖）。
    if zip_path.exists() {
        fs::remove_file(zip_path)
            .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("无法覆盖旧快照: {e}")))?;
    }
    fs::rename(&tmp_path, zip_path)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("快照改名失败: {e}")))?;
    Ok(())
}

fn open_zip(zip_path: &Path) -> Result<zip::ZipArchive<fs::File>> {
    if !zip_path.is_file() {
        return Err(Error::new(
            ErrorCode::SnapshotNotFound,
            format!("快照不存在: {}", zip_path.display()),
        ));
    }
    let file = fs::File::open(zip_path)
        .map_err(|e| Error::new(ErrorCode::SnapshotNotFound, format!("快照不可读: {e}")))?;
    zip::ZipArchive::new(file)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("zip 解析失败: {e}")))
}

fn read_entry(archive: &mut zip::ZipArchive<fs::File>, name: &str) -> Result<Vec<u8>> {
    let mut f = archive
        .by_name(name)
        .map_err(|_| Error::new(ErrorCode::SnapshotInvalid, format!("包内缺少条目: {name}")))?;
    let mut out = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut out).map_err(|e| {
        Error::new(
            ErrorCode::SnapshotInvalid,
            format!("读取条目失败 {name}: {e}"),
        )
    })?;
    Ok(out)
}

/// 条目路径安全检查：必须以 `data/` 开头；拒绝绝对路径、`..`、反斜杠、盘符
/// （zip-slip 防线；落盘全部为普通文件，不创建链接）。返回 `data/` 后的相对路径。
fn safe_entry_rel(p: &str) -> Option<String> {
    let rest = p.strip_prefix(DATA_PREFIX)?;
    if rest.is_empty() || p.contains('\\') || p.contains(':') || p.starts_with('/') {
        return None;
    }
    let mut depth: usize = 0;
    for comp in rest.split('/') {
        match comp {
            "" | "." => {}
            ".." => return None,
            _ => depth += 1,
        }
    }
    if depth == 0 {
        None
    } else {
        Some(rest.to_string())
    }
}

fn extract_all(zip_path: &Path, target_dir: &Path, manifest: &SnapshotManifest) -> Result<()> {
    let mut archive = open_zip(zip_path)?;
    fs::create_dir_all(target_dir)
        .map_err(|e| Error::new(ErrorCode::SnapshotInvalid, format!("无法创建目标目录: {e}")))?;
    for entry in &manifest.entries {
        let rel = safe_entry_rel(&entry.path).ok_or_else(|| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("条目路径不安全: {}", entry.path),
            )
        })?;
        let bytes = read_entry(&mut archive, &entry.path)?;
        if bytes.len() as u64 != entry.bytes || sha256_hex(&bytes) != entry.sha256 {
            return Err(Error::new(
                ErrorCode::SnapshotInvalid,
                format!("条目哈希不符: {}", entry.path),
            ));
        }
        let abs = target_dir.join(&rel);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent).map_err(|e| {
                Error::new(
                    ErrorCode::SnapshotInvalid,
                    format!("无法创建目录 {}: {e}", parent.display()),
                )
            })?;
        }
        fs::write(&abs, &bytes).map_err(|e| {
            Error::new(
                ErrorCode::SnapshotInvalid,
                format!("写入失败 {}: {e}", entry.path),
            )
        })?;
    }
    Ok(())
}

/// 目标目录当前文件集合（递归普通文件，`/` 分隔相对路径，排序）。
fn current_files(target_dir: &Path) -> Result<Vec<String>> {
    let mut out: Vec<(String, PathBuf)> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    if target_dir.is_dir() {
        collect_files(target_dir, target_dir, &mut out, &mut warnings)?;
    }
    let _ = warnings;
    Ok(out.into_iter().map(|(rel, _)| rel).collect())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut s = String::with_capacity(digest.len() * 2);
    for b in digest {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-snap-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write_file(path: &Path, content: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(path, content).unwrap();
    }

    #[test]
    fn create_list_round_trip() {
        let root = temp_dir("roundtrip");
        let data = root.join("data/db");
        write_file(&data.join("a.txt"), "hello");
        write_file(&data.join("sub/b.txt"), "world");
        let out = root.join("snapshots/app-db");
        let meta = create_snapshot(&data, &out, "app-db", Some("api"), "初次").unwrap();
        assert_eq!(meta.file_count, 2);
        assert_eq!(meta.total_bytes, 10);
        assert_eq!(meta.note, "初次");
        assert!(out.join(format!("{}.zip", meta.created_at)).is_file());

        let (list, warnings) = list_snapshots(&out);
        assert!(warnings.is_empty());
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, meta.id);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn create_rejects_missing_dir() {
        let root = temp_dir("missing");
        let err =
            create_snapshot(&root.join("nope"), &root.join("out"), "v", None, "").unwrap_err();
        assert_eq!(err.code(), ErrorCode::SnapshotInvalid);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn empty_dir_snapshot_allowed() {
        let root = temp_dir("empty");
        let data = root.join("data");
        fs::create_dir_all(&data).unwrap();
        let out = root.join("snapshots/v");
        let meta = create_snapshot(&data, &out, "v", None, "干净状态").unwrap();
        assert_eq!(meta.file_count, 0);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_replaces_content_and_removes_strays() {
        let root = temp_dir("restore");
        let data = root.join("data");
        write_file(&data.join("keep.txt"), "v1");
        let out = root.join("snapshots/v");
        let meta = create_snapshot(&data, &out, "v", None, "").unwrap();

        // 演化目录：修改 keep.txt、新增 stray.log
        write_file(&data.join("keep.txt"), "v2-changed");
        write_file(&data.join("stray.log"), "junk");

        let preview =
            restore_preview(&out.join(format!("{}.zip", meta.created_at)), &data).unwrap();
        assert!(preview.target_exists);
        assert_eq!(preview.current_files, 2);
        assert_eq!(preview.remove_count, 1);
        assert_eq!(preview.remove_sample, vec!["stray.log".to_string()]);

        let stash = out.join(".stash-test");
        let outcome =
            restore_snapshot(&out.join(format!("{}.zip", meta.created_at)), &data, &stash).unwrap();
        assert_eq!(outcome.restored_files, 1);
        assert_eq!(outcome.removed_files, 1);
        assert_eq!(fs::read_to_string(data.join("keep.txt")).unwrap(), "v1");
        assert!(!data.join("stray.log").exists());
        assert!(!stash.exists(), "成功后 stash 应被清理");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_into_missing_target() {
        let root = temp_dir("restore-missing");
        let data = root.join("data");
        write_file(&data.join("a.txt"), "x");
        let out = root.join("snapshots/v");
        let meta = create_snapshot(&data, &out, "v", None, "").unwrap();
        fs::remove_dir_all(&data).unwrap();
        let stash = out.join(".stash-test");
        let outcome =
            restore_snapshot(&out.join(format!("{}.zip", meta.created_at)), &data, &stash).unwrap();
        assert_eq!(outcome.restored_files, 1);
        assert_eq!(outcome.removed_files, 0);
        assert_eq!(fs::read_to_string(data.join("a.txt")).unwrap(), "x");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_corrupt_zip_rolls_back() {
        let root = temp_dir("rollback");
        let data = root.join("data");
        write_file(&data.join("a.txt"), "original");
        let out = root.join("snapshots/v");
        let meta = create_snapshot(&data, &out, "v", None, "").unwrap();
        let zip = out.join(format!("{}.zip", meta.created_at));

        // 破坏 zip 字节（模拟损坏）
        let mut raw = fs::read(&zip).unwrap();
        let mid = raw.len() / 2;
        raw[mid] = raw[mid].wrapping_add(0xff);
        fs::write(&zip, &raw).unwrap();

        let stash = out.join(".stash-test");
        let err = restore_snapshot(&zip, &data, &stash).unwrap_err();
        assert_eq!(err.code(), ErrorCode::SnapshotInvalid);
        // 目标目录未被破坏
        assert_eq!(fs::read_to_string(data.join("a.txt")).unwrap(), "original");
        assert!(!stash.exists(), "失败回滚后 stash 不应残留");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn restore_rejects_zip_slip_entry() {
        let root = temp_dir("zipslip");
        let out = root.join("snapshots/v");
        fs::create_dir_all(&out).unwrap();
        let zip = out.join("123.zip");
        write_test_zip(
            &zip,
            &[(
                MANIFEST_NAME,
                serde_json::to_vec(&manifest_json(vec![SnapshotEntry {
                    path: "data/../escape.txt".into(),
                    sha256: sha256_hex(b"x"),
                    bytes: 1,
                }]))
                .unwrap(),
            )],
        );
        let target = root.join("target");
        let err = restore_snapshot(&zip, &target, &out.join(".stash")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::SnapshotInvalid);
        assert!(!root.join("escape.txt").exists());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn verify_rejects_bad_hash_and_future_format() {
        let root = temp_dir("verify");
        let out = root.join("snapshots/v");
        fs::create_dir_all(&out).unwrap();

        // 哈希不符
        let zip = out.join("111.zip");
        write_test_zip(
            &zip,
            &[
                (
                    MANIFEST_NAME,
                    serde_json::to_vec(&manifest_json(vec![SnapshotEntry {
                        path: "data/a.txt".into(),
                        sha256: "00".repeat(32),
                        bytes: 1,
                    }]))
                    .unwrap(),
                ),
                ("data/a.txt", b"x".to_vec()),
            ],
        );
        assert_eq!(
            verify_snapshot(&zip).unwrap_err().code(),
            ErrorCode::SnapshotInvalid
        );

        // 未来 format
        let zip2 = out.join("222.zip");
        let mut m = manifest_json(vec![]);
        m.format = SNAPSHOT_FORMAT + 1;
        write_test_zip(&zip2, &[(MANIFEST_NAME, serde_json::to_vec(&m).unwrap())]);
        assert_eq!(
            verify_snapshot(&zip2).unwrap_err().code(),
            ErrorCode::SnapshotVersion
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn list_skips_corrupt_with_warning() {
        let root = temp_dir("list-corrupt");
        let out = root.join("snapshots/v");
        fs::create_dir_all(&out).unwrap();
        fs::write(out.join("999.zip"), b"not a zip").unwrap();
        let (list, warnings) = list_snapshots(&out);
        assert!(list.is_empty());
        assert_eq!(warnings.len(), 1);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn delete_missing_is_snapshot_not_found() {
        let root = temp_dir("delete");
        let err = delete_snapshot(&root.join("nope.zip")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::SnapshotNotFound);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn caps_reject_oversize() {
        assert!(check_caps(10, 100).is_ok());
        assert_eq!(
            check_caps(MAX_SNAPSHOT_ENTRIES + 1, 0).unwrap_err().code(),
            ErrorCode::SnapshotInvalid
        );
        assert_eq!(
            check_caps(0, MAX_SNAPSHOT_TOTAL_BYTES + 1)
                .unwrap_err()
                .code(),
            ErrorCode::SnapshotInvalid
        );
    }

    #[test]
    fn safe_entry_rel_rules() {
        assert_eq!(safe_entry_rel("data/a.txt").as_deref(), Some("a.txt"));
        assert_eq!(
            safe_entry_rel("data/sub/a.txt").as_deref(),
            Some("sub/a.txt")
        );
        assert!(safe_entry_rel("data/").is_none());
        assert!(safe_entry_rel("manifest.json").is_none());
        assert!(safe_entry_rel("data/../x").is_none());
        assert!(safe_entry_rel("data/a\\b").is_none());
        assert!(safe_entry_rel("data/C:/x").is_none());
    }

    // ---- 测试助手：手工打包任意条目（构造恶意/畸形包用），同 pkg.rs ----

    fn write_test_zip(dest: &Path, entries: &[(&str, Vec<u8>)]) {
        let file = fs::File::create(dest).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            w.start_file(*name, opts).unwrap();
            use std::io::Write as _;
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    fn manifest_json(entries: Vec<SnapshotEntry>) -> SnapshotManifest {
        SnapshotManifest {
            format: SNAPSHOT_FORMAT,
            volume_id: "v".into(),
            service: None,
            note: String::new(),
            created_at: 0,
            source_os: "windows".into(),
            app_version: "0.0.0".into(),
            file_count: entries.len() as u64,
            total_bytes: entries.iter().map(|e| e.bytes).sum(),
            entries,
        }
    }
}
