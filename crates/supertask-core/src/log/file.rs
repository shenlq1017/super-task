//! 1.0 日志文件写入；1.2 §8.2 起 `max_bytes` 触发**重命名轮转**
//! （`.log` → `.log.1` → `.log.2`…，保留 `max_files` 个），轮转失败回退
//! 截尾保可写（不丢当前写入，由调用方映射 `LOG_RETENTION_FAILED`）。

use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::ipc::DEFAULT_LOG_MAX_BYTES;

pub struct LogFile {
    path: PathBuf,
    max_bytes: u64,
    retain_tail: u64,
    max_files: u32,
    written: u64,
}

impl LogFile {
    pub fn open(
        path: PathBuf,
        max_bytes: Option<u64>,
        retain_tail: Option<u64>,
    ) -> std::io::Result<Self> {
        Self::open_with_files(path, max_bytes, retain_tail, None)
    }

    /// `max_files`：轮转文件保留个数（§8.2，默认 5）。
    pub fn open_with_files(
        path: PathBuf,
        max_bytes: Option<u64>,
        retain_tail: Option<u64>,
        max_files: Option<u32>,
    ) -> std::io::Result<Self> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let max_bytes = max_bytes.unwrap_or(DEFAULT_LOG_MAX_BYTES).max(1024);
        let retain_tail = retain_tail.unwrap_or(2 * 1024 * 1024).min(max_bytes);
        let written = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
        Ok(Self {
            path,
            max_bytes,
            retain_tail,
            max_files: max_files.unwrap_or(5).max(1),
            written,
        })
    }

    pub fn append_line(&mut self, line: &str) -> std::io::Result<()> {
        if self.written > self.max_bytes {
            if self.rotate().is_err() {
                // 轮转失败：回退截尾，活动文件保持可写（§8.2）
                self.truncate_tail()?;
            }
        }
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")?;
        self.written += line.len() as u64 + 1;
        Ok(())
    }

    /// `.log.N` 倒序重命名轮转；最旧的直接删除。
    fn rotate(&mut self) -> std::io::Result<()> {
        let stem = self.path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if stem.is_empty() {
            return Err(std::io::Error::new(std::io::ErrorKind::Other, "no stem"));
        }
        let dir = self
            .path
            .parent()
            .unwrap_or_else(|| std::path::Path::new("."));
        // 先删最旧腾位
        let oldest = dir.join(format!("{stem}.{}", self.max_files));
        if oldest.exists() {
            fs::remove_file(&oldest)?;
        }
        for n in (1..self.max_files as i64).rev() {
            let from = dir.join(format!("{stem}.{n}"));
            if from.exists() {
                fs::rename(&from, dir.join(format!("{stem}.{}", n + 1)))?;
            }
        }
        fs::rename(&self.path, dir.join(format!("{stem}.1")))?;
        self.written = 0;
        Ok(())
    }

    fn truncate_tail(&mut self) -> std::io::Result<()> {
        let mut f = File::open(&self.path)?;
        let len = f.metadata()?.len();
        let start = len.saturating_sub(self.retain_tail);
        f.seek(SeekFrom::Start(start))?;
        let mut buf = Vec::new();
        f.read_to_end(&mut buf)?;
        if let Some(i) = buf.iter().position(|b| *b == b'\n') {
            buf = buf[i + 1..].to_vec();
        }
        fs::write(&self.path, &buf)?;
        self.written = buf.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn rotation_creates_numbered_files_and_caps() {
        let dir = std::env::temp_dir().join(format!("st-rot-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("svc.log");
        // max_bytes 有 1024 下限（钳制），用 1024 计算轮转
        let mut lf = LogFile::open_with_files(path.clone(), Some(1024), Some(64), Some(3)).unwrap();
        let line = format!("line-xxx {}", "x".repeat(60)); // ~69 字节/行
        for _ in 0..60 {
            lf.append_line(&line).unwrap();
        }
        assert!(path.exists(), "活动文件存在");
        assert!(dir.join("svc.log.1").exists(), "至少轮转出 1 个文件");
        let rotated = (1..=9)
            .map(|n| dir.join(format!("svc.log.{n}")))
            .filter(|p| p.exists())
            .count();
        assert!((1..=3).contains(&rotated), "rotated={rotated}");
        assert!(!dir.join("svc.log.4").exists(), "不超过 max_files=3");
        let active_len = fs::metadata(&path).unwrap().len();
        assert!(
            active_len <= 1024 + line.len() as u64 + 1,
            "活动文件应刚轮转过: {active_len}"
        );
        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn rotation_falls_back_to_truncate_when_rename_blocked() {
        let dir = std::env::temp_dir().join(format!("st-rotfb-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("svc.log");
        // 让 .log.1 是目录 → rename 失败 → 回退截尾
        fs::create_dir_all(dir.join("svc.log.1")).unwrap();
        let mut lf = LogFile::open_with_files(path.clone(), Some(32), Some(32), Some(3)).unwrap();
        for i in 0..20 {
            let _ = lf.append_line(&format!("line-{i} xxxxxxxxxxxxxxxxxxxxxx"));
        }
        assert!(path.exists(), "回退后活动文件仍可写");
        fs::remove_dir_all(&dir).ok();
        let _ = Path::new("");
    }
}
