use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;

use crate::ipc::DEFAULT_LOG_MAX_BYTES;

pub struct LogFile {
    path: PathBuf,
    max_bytes: u64,
    retain_tail: u64,
    written: u64,
}

impl LogFile {
    pub fn open(path: PathBuf, max_bytes: Option<u64>, retain_tail: Option<u64>) -> std::io::Result<Self> {
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
            written,
        })
    }

    pub fn append_line(&mut self, line: &str) -> std::io::Result<()> {
        if self.written > self.max_bytes {
            self.truncate_tail()?;
        }
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
        writeln!(f, "{line}")?;
        self.written += line.len() as u64 + 1;
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
