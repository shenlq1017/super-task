//! 1.2 日志历史搜索、导出与保留（规格 §8）。
//!
//! 文件布局沿用 1.0：`.supertask/logs/{id}.log[.N]`、`scripts/{id}.log[.N]`、
//! `system.log`。`.log` 是活动文件，`.log.N` 数字越大越旧。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use crate::error::{Error, ErrorCode, Result};
use crate::ipc::{LogSource, LogSourceKind};
use crate::spec::LogRetentionSpec;

const MAX_QUERY_CHARS: usize = 256;
pub const DEFAULT_SEARCH_LIMIT: usize = 200;
pub const MAX_SEARCH_LIMIT: usize = 5000;
const LOGS_DIR: &str = ".supertask/logs";

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchHit {
    pub kind: String,
    pub id: String,
    /// 文件名（含轮转序号），不含目录。
    pub file: String,
    pub line_no: usize,
    pub text: String,
    /// 无可解析日期时为 null（1.0 行格式不带时间戳）。
    pub ts: Option<u64>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SearchResult {
    pub items: Vec<SearchHit>,
    pub truncated: bool,
    /// 实际读到并扫描过的历史文件数；为 0 表示该范围还没有可搜索的历史。
    pub files_scanned: usize,
}

/// 一个源（服务/脚本/system）的文件列表：活动文件在前，轮转文件从新到旧。
fn source_files(root: &Path, source: &LogSource) -> Vec<PathBuf> {
    let base = root.join(LOGS_DIR);
    let (stem, dir) = match source.kind {
        LogSourceKind::Script => (format!("{}.log", source.id), base.join("scripts")),
        LogSourceKind::System => ("system.log".to_string(), base.clone()),
        LogSourceKind::Service => (format!("{}.log", source.id), base.clone()),
    };
    let active = dir.join(&stem);
    let mut files = vec![active];
    for n in 1..=64 {
        let rotated = dir.join(format!("{stem}.{n}"));
        if rotated.exists() {
            files.push(rotated);
        } else {
            break;
        }
    }
    files
}

fn all_sources(root: &Path) -> Vec<LogSource> {
    let base = root.join(LOGS_DIR);
    let mut out = Vec::new();
    if let Ok(entries) = fs::read_dir(&base) {
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter(|e| e.path().is_file())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".log").map(str::to_string)
            })
            .filter(|n| n != "system")
            .collect();
        ids.sort();
        for id in ids {
            out.push(LogSource { kind: LogSourceKind::Service, id });
        }
    }
    if let Ok(entries) = fs::read_dir(base.join("scripts")) {
        let mut ids: Vec<String> = entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let name = e.file_name().to_string_lossy().into_owned();
                name.strip_suffix(".log").map(str::to_string)
            })
            .collect();
        ids.sort();
        for id in ids {
            out.push(LogSource { kind: LogSourceKind::Script, id });
        }
    }
    if base.join("system.log").exists() {
        out.push(LogSource { kind: LogSourceKind::System, id: "system".into() });
    }
    out
}

/// literal 搜索（无正则）。query ≤256 字符；limit 缺省 200、上限 5000。
/// 活动文件 → 最旧轮转文件顺序流式读取；超 limit 截断并标记 truncated。
pub fn search_logs(
    root: &Path,
    source: Option<&LogSource>,
    query: &str,
    case_sensitive: bool,
    limit: Option<usize>,
) -> Result<SearchResult> {
    if query.chars().count() > MAX_QUERY_CHARS {
        return Err(Error::new(
            ErrorCode::LogQueryInvalid,
            format!("query 最长 {MAX_QUERY_CHARS} 字符"),
        ));
    }
    let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT).min(MAX_SEARCH_LIMIT);
    let needle = if case_sensitive { query.to_string() } else { query.to_lowercase() };

    let sources: Vec<LogSource> = match source {
        Some(s) => vec![LogSource { kind: s.kind.clone(), id: s.id.clone() }],
        None => all_sources(root),
    };
    let mut items = Vec::new();
    let mut truncated = false;
    let mut files_scanned = 0usize;
    'outer: for src in sources {
        for file in source_files(root, &src) {
            let Ok(text) = fs::read_to_string(&file) else { continue };
            files_scanned += 1;
            let fname = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
            for (idx, line) in text.lines().enumerate() {
                let matched = if case_sensitive {
                    line.contains(&needle)
                } else {
                    line.to_lowercase().contains(&needle)
                };
                if !matched {
                    continue;
                }
                if items.len() >= limit {
                    truncated = true;
                    break 'outer;
                }
                items.push(SearchHit {
                    kind: match src.kind {
                        LogSourceKind::Script => "script".into(),
                        LogSourceKind::System => "system".into(),
                        LogSourceKind::Service => "service".into(),
                    },
                    id: src.id.clone(),
                    file: fname.clone(),
                    line_no: idx + 1,
                    text: line.to_string(),
                    ts: None,
                });
            }
        }
    }
    Ok(SearchResult { items, truncated, files_scanned })
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TailHit {
    pub kind: String,
    pub id: String,
    pub file: String,
    pub line_no: usize,
    pub text: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct TailResult {
    pub items: Vec<TailHit>,
    /// 实际行数超过请求行数时为 true（items 只保留最后 lines 行）。
    pub truncated: bool,
    pub files_scanned: usize,
}

/// 1.5 CLI/MCP：历史日志尾部（只读 `.supertask/logs`，不要求持有工作区锁）。
/// 与 search 同一文件遍历顺序（活动文件在前、轮转从新到旧），收集全部行后取末尾。
pub fn tail_logs(root: &Path, source: Option<&LogSource>, lines: usize) -> Result<TailResult> {
    let lines = lines.max(1).min(MAX_SEARCH_LIMIT);
    let sources: Vec<LogSource> = match source {
        Some(s) => vec![LogSource { kind: s.kind.clone(), id: s.id.clone() }],
        None => all_sources(root),
    };
    let mut items: Vec<TailHit> = Vec::new();
    let mut files_scanned = 0usize;
    for src in &sources {
        for file in source_files(root, src) {
            let Ok(text) = fs::read_to_string(&file) else { continue };
            files_scanned += 1;
            let fname = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
            for (idx, line) in text.lines().enumerate() {
                items.push(TailHit {
                    kind: match src.kind {
                        LogSourceKind::Script => "script".into(),
                        LogSourceKind::System => "system".into(),
                        LogSourceKind::Service => "service".into(),
                    },
                    id: src.id.clone(),
                    file: fname.clone(),
                    line_no: idx + 1,
                    text: line.to_string(),
                });
            }
        }
    }
    let truncated = items.len() > lines;
    if truncated {
        items.drain(..items.len() - lines);
    }
    Ok(TailResult { items, truncated, files_scanned })
}

/// §8.4 导出。format: text | jsonl；目标已存在 → 拒绝（不覆盖）。
/// 返回导出行数。范围与 search 相同（query 可为 None = 全部）。
pub fn export_logs(
    root: &Path,
    source: Option<&LogSource>,
    query: Option<&str>,
    case_sensitive: bool,
    format: &str,
    dest: &Path,
) -> Result<usize> {
    match format {
        "text" | "jsonl" => {}
        other => {
            return Err(Error::new(
                ErrorCode::LogQueryInvalid,
                format!("format 只支持 text/jsonl，收到 {other:?}"),
            ))
        }
    }
    if dest.exists() {
        return Err(Error::new(
            ErrorCode::LogExportFailed,
            format!("目标文件已存在，不覆盖: {}", dest.display()),
        ));
    }
    if let Some(parent) = dest.parent() {
        if !parent.is_dir() {
            return Err(Error::new(
                ErrorCode::LogExportFailed,
                format!("目标目录不存在: {}", parent.display()),
            ));
        }
    }
    let result = match query {
        Some(q) if !q.is_empty() => search_logs(root, source, q, case_sensitive, Some(MAX_SEARCH_LIMIT))?,
        _ => {
            // 无 query：导出范围内全部行
            let sources: Vec<LogSource> = match source {
                Some(s) => vec![LogSource { kind: s.kind.clone(), id: s.id.clone() }],
                None => all_sources(root),
            };
            let mut items = Vec::new();
            let mut files_scanned = 0usize;
            for src in sources {
                for file in source_files(root, &src) {
                    let Ok(text) = fs::read_to_string(&file) else { continue };
                    files_scanned += 1;
                    let fname = file.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    for (idx, line) in text.lines().enumerate() {
                        items.push(SearchHit {
                            kind: match src.kind {
                                LogSourceKind::Script => "script".into(),
                                LogSourceKind::System => "system".into(),
                                LogSourceKind::Service => "service".into(),
                            },
                            id: src.id.clone(),
                            file: fname.clone(),
                            line_no: idx + 1,
                            text: line.to_string(),
                            ts: None,
                        });
                    }
                }
            }
            SearchResult { items, truncated: false, files_scanned }
        }
    };
    let mut f = fs::File::create(dest)
        .map_err(|e| Error::new(ErrorCode::LogExportFailed, format!("无法创建导出文件: {e}")))?;
    for hit in &result.items {
        let line = if format == "jsonl" {
            format!(
                "{{\"kind\":\"{}\",\"id\":\"{}\",\"file\":\"{}\",\"line_no\":{},\"ts\":{},\"text\":\"{}\"}}",
                json_escape(&hit.kind),
                json_escape(&hit.id),
                json_escape(&hit.file),
                hit.line_no,
                hit.ts.map(|t| t.to_string()).unwrap_or_else(|| "null".into()),
                json_escape(&hit.text)
            )
        } else {
            hit.text.clone()
        };
        writeln!(f, "{line}")
            .map_err(|e| Error::new(ErrorCode::LogExportFailed, format!("写入导出失败: {e}")))?;
    }
    Ok(result.items.len())
}

/// 最小 JSON 字符串转义（core 不依赖 serde_json）。
fn json_escape(s: &str) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(s.len() + 2);
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let code = c as u32;
                out.push_str("\\u00");
                out.push(HEX[(code >> 4) as usize & 0xf] as char);
                out.push(HEX[code as usize & 0xf] as char);
            }
            c => out.push(c),
        }
    }
    out
}

#[derive(Debug, Clone, Default, serde::Serialize)]
pub struct RetentionSummary {
    pub deleted_files: usize,
    pub deleted_bytes: u64,
}

fn rotated_files(dir: &Path, stem: &str) -> Vec<(PathBuf, u64)> {
    // 返回 .log.1..N 按 N 升序（N 大 = 旧）
    let mut out = Vec::new();
    for n in 1..=9999 {
        let p = dir.join(format!("{stem}.{n}"));
        if let Ok(meta) = fs::metadata(&p) {
            out.push((p, meta.len()));
        } else {
            break;
        }
    }
    out
}

/// §8.2 保留清理：先删超龄，再按每源 max_files 删最旧，最后按总大小删最旧。
/// 活动 `.log` 与 `system.log` 永不直接删除。
pub fn run_retention(root: &Path, retention: Option<&LogRetentionSpec>) -> Result<RetentionSummary> {
    let max_files = retention.and_then(|r| r.max_files).unwrap_or(5).max(1);
    let max_age_days = retention.and_then(|r| r.max_age_days);
    let max_total = retention.and_then(|r| r.max_total_bytes);
    let logs_dir = root.join(LOGS_DIR);
    let mut summary = RetentionSummary::default();
    if !logs_dir.is_dir() {
        return Ok(summary);
    }
    let now = std::time::SystemTime::now();

    // 收集 (path, size, mtime, is_active, age_expired)
    let mut all: Vec<(PathBuf, u64, std::time::SystemTime, bool)> = Vec::new();
    let push_entry = |p: PathBuf, active: bool, all: &mut Vec<(PathBuf, u64, std::time::SystemTime, bool)>| {
        if let Ok(meta) = fs::metadata(&p) {
            let mtime = meta.modified().unwrap_or(now);
            all.push((p, meta.len(), mtime, active));
        }
    };
    if let Ok(entries) = fs::read_dir(&logs_dir) {
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let active = name.ends_with(".log");
            push_entry(p, active, &mut all);
        }
    }
    let scripts_dir = logs_dir.join("scripts");
    if let Ok(entries) = fs::read_dir(&scripts_dir) {
        for e in entries.filter_map(|e| e.ok()) {
            let p = e.path();
            if !p.is_file() {
                continue;
            }
            let name = e.file_name().to_string_lossy().into_owned();
            let active = name.ends_with(".log");
            push_entry(p, active, &mut all);
        }
    }

    let mut expired: Vec<PathBuf> = Vec::new();
    if let Some(days) = max_age_days {
        let cutoff = now - std::time::Duration::from_secs(days as u64 * 86_400);
        for (p, _, mtime, active) in &all {
            if !active && *mtime < cutoff {
                expired.push(p.clone());
            }
        }
    }
    let expired_set: std::collections::HashSet<PathBuf> = expired.iter().cloned().collect();
    for p in &expired {
        if let Ok(meta) = fs::metadata(p) {
            let size = meta.len();
            if fs::remove_file(p).is_ok() {
                summary.deleted_files += 1;
                summary.deleted_bytes += size;
            }
        }
    }
    let alive: Vec<(PathBuf, u64, std::time::SystemTime, bool)> = all
        .into_iter()
        .filter(|(p, ..)| !expired_set.contains(p))
        .collect();

    // 每源轮转文件数上限：同 stem 的 .log.N 超过 max_files 删最旧
    let mut stems: Vec<(PathBuf, String)> = Vec::new(); // (dir, stem)
    for (p, _, _, _) in &alive {
        if let Some(dir) = p.parent() {
            if let Some(name) = p.file_name().map(|n| n.to_string_lossy().into_owned()) {
                // stem = 含 .log 的前缀（"api.log.3" → "api.log"），与 LogFile 轮转命名一致
                let stem = if name.ends_with(".log") {
                    name.clone()
                } else if let Some(idx) = name.rfind(".log.") {
                    name[..idx + 5].to_string()
                } else {
                    continue;
                };
                if !stems.iter().any(|(d, s)| *d == dir && *s == stem) {
                    stems.push((dir.to_path_buf(), stem));
                }
            }
        }
    }
    let mut deleted_now: std::collections::HashSet<PathBuf> = expired_set;
    for (dir, stem) in &stems {
        let rotated = rotated_files(dir, stem);
        // rotated 升序（新→旧），保留前 max_files 个
        if rotated.len() > max_files as usize {
            for (p, _) in rotated.iter().skip(max_files as usize) {
                if deleted_now.insert(p.clone()) {
                    if let Ok(meta) = fs::metadata(p) {
                        let size = meta.len();
                        if fs::remove_file(p).is_ok() {
                            summary.deleted_files += 1;
                            summary.deleted_bytes += size;
                        }
                    }
                }
            }
        }
    }
    // 总量上限：从最旧轮转文件开始删
    if let Some(cap) = max_total {
        let mut live: Vec<(PathBuf, u64)> = Vec::new();
        for (dir, stem) in &stems {
            for (p, size) in rotated_files(dir, stem) {
                if !deleted_now.contains(&p) {
                    live.push((p, size));
                }
            }
        }
        // 旧→新排序：N 越大越旧；rotated_files 返回升序（新→旧），反转
        live.reverse();
        let mut total: u64 = alive
            .iter()
            .filter(|(p, ..)| !deleted_now.contains(p))
            .map(|(_, size, _, _)| *size)
            .sum();
        for (p, size) in live {
            if total <= cap {
                break;
            }
            if fs::remove_file(&p).is_ok() {
                total = total.saturating_sub(size);
                summary.deleted_files += 1;
                summary.deleted_bytes += size;
            }
        }
    }
    Ok(summary)
}

/// 纯函数：超龄判定（便于测试，不碰文件 mtime）。
pub fn is_expired(mtime: std::time::SystemTime, now: std::time::SystemTime, days: u32) -> bool {
    match now.duration_since(mtime) {
        Ok(age) => age >= std::time::Duration::from_secs(days as u64 * 86_400),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_root(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-logsrch-{tag}-{}", std::process::id()));
        fs::create_dir_all(dir.join(".supertask/logs/scripts")).unwrap();
        dir
    }

    #[test]
    fn search_filters_by_query_and_source_order() {
        let root = temp_root("a");
        fs::write(root.join(".supertask/logs/api.log"), "INFO start\nERROR boom\nINFO ok\n").unwrap();
        fs::write(root.join(".supertask/logs/api.log.1"), "ERROR old-boom\n").unwrap();
        fs::write(root.join(".supertask/logs/scripts/build.log"), "error lowercase\n").unwrap();
        fs::write(root.join(".supertask/logs/system.log"), "SYS error\n").unwrap();

        // 全源搜 error（小写不敏感）：活动→轮转顺序，服务在 system/script 前
        let r = search_logs(&root, None, "error", false, None).unwrap();
        assert_eq!(r.items.len(), 4);
        assert_eq!(r.items[0].text, "ERROR boom");
        assert_eq!(r.items[1].file, "api.log.1", "活动文件之后是最近的轮转文件");
        assert!(!r.truncated);

        // 指定源 + 大小写敏感
        let src = LogSource { kind: LogSourceKind::Service, id: "api".into() };
        let r2 = search_logs(&root, Some(&src), "ERROR", true, None).unwrap();
        assert_eq!(r2.items.len(), 2);
        assert_eq!(r2.items[1].line_no, 1);

        // limit 截断
        let r3 = search_logs(&root, None, "error", false, Some(2)).unwrap();
        assert!(r3.truncated && r3.items.len() == 2);

        // query 超长
        let long = "x".repeat(257);
        assert_eq!(
            search_logs(&root, None, &long, false, None).unwrap_err().code(),
            ErrorCode::LogQueryInvalid
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn export_text_and_jsonl_never_overwrite() {
        let root = temp_root("b");
        fs::write(root.join(".supertask/logs/api.log"), "one\ntwo ERROR\n").unwrap();
        let dest = root.join("export.txt");
        let n = export_logs(&root, None, Some("ERROR"), false, "text", &dest).unwrap();
        assert_eq!(n, 1);
        assert_eq!(fs::read_to_string(&dest).unwrap(), "two ERROR\n");
        assert_eq!(
            export_logs(&root, None, Some("ERROR"), false, "text", &dest)
                .unwrap_err()
                .code(),
            ErrorCode::LogExportFailed
        );
        let dest2 = root.join("export.jsonl");
        export_logs(&root, None, None, false, "jsonl", &dest2).unwrap();
        let body = fs::read_to_string(&dest2).unwrap();
        assert!(body.contains("\"line_no\":1"));
        assert!(body.contains("api.log"));
        assert_eq!(
            export_logs(&root, None, None, false, "csv", &dest).unwrap_err().code(),
            ErrorCode::LogQueryInvalid
        );
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn retention_caps_files_and_total_never_active() {
        let root = temp_root("c");
        fs::write(root.join(".supertask/logs/api.log"), "active\n").unwrap();
        for n in 1..=4 {
            fs::write(root.join(format!(".supertask/logs/api.log.{n}")), "x".repeat(1000)).unwrap();
        }
        fs::write(root.join(".supertask/logs/system.log"), "sys\n").unwrap();
        let retention = LogRetentionSpec {
            max_files: Some(2),
            max_age_days: None,
            max_total_bytes: None,
            extra: Default::default(),
        };
        let s = run_retention(&root, Some(&retention)).unwrap();
        // api.log.3 / api.log.4 被删（保留 2 个轮转），活动与 system 不动
        assert_eq!(s.deleted_files, 2);
        assert!(root.join(".supertask/logs/api.log").exists());
        assert!(root.join(".supertask/logs/api.log.1").exists());
        assert!(!root.join(".supertask/logs/api.log.3").exists());
        assert!(root.join(".supertask/logs/system.log").exists());

        // 总量上限：剩余总量 1000+1000+4 > 2000 → 从最旧开始删
        let retention2 = LogRetentionSpec {
            max_files: Some(5),
            max_age_days: None,
            max_total_bytes: Some(2000),
            extra: Default::default(),
        };
        let s2 = run_retention(&root, Some(&retention2)).unwrap();
        assert!(s2.deleted_files >= 1);
        assert!(root.join(".supertask/logs/api.log").exists(), "活动文件永不被清理删除");
        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn expiry_logic() {
        let now = std::time::SystemTime::now();
        let old = now - std::time::Duration::from_secs(9 * 86_400);
        let fresh = now - std::time::Duration::from_secs(3600);
        assert!(is_expired(old, now, 7));
        assert!(!is_expired(fresh, now, 7));
    }
}
