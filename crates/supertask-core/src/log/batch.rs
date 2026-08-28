use crate::ipc::LOG_BATCH_ITEMS;
use crate::log::LogLine;

pub struct LogBatcher {
    buf: Vec<LogLine>,
    first_ts_ms: Option<u64>,
    max_items: usize,
    max_wait_ms: u64,
}

impl Default for LogBatcher {
    fn default() -> Self {
        Self {
            buf: Vec::new(),
            first_ts_ms: None,
            max_items: LOG_BATCH_ITEMS,
            max_wait_ms: crate::ipc::LOG_BATCH_MS,
        }
    }
}

impl LogBatcher {
    pub fn push(&mut self, now_ms: u64, line: LogLine) -> Option<Vec<LogLine>> {
        if self.buf.is_empty() {
            self.first_ts_ms = Some(now_ms);
        }
        self.buf.push(line);
        if self.buf.len() >= self.max_items {
            return Some(self.take());
        }
        None
    }

    pub fn poll_flush(&mut self, now_ms: u64) -> Option<Vec<LogLine>> {
        if should_flush_batch(self.buf.len(), self.first_ts_ms, now_ms, self.max_items, self.max_wait_ms)
        {
            Some(self.take())
        } else {
            None
        }
    }

    fn take(&mut self) -> Vec<LogLine> {
        self.first_ts_ms = None;
        std::mem::take(&mut self.buf)
    }
}

pub fn should_flush_batch(
    len: usize,
    first_ts_ms: Option<u64>,
    now_ms: u64,
    max_items: usize,
    max_wait_ms: u64,
) -> bool {
    if len == 0 {
        return false;
    }
    if len >= max_items {
        return true;
    }
    first_ts_ms
        .map(|t| now_ms.saturating_sub(t) >= max_wait_ms)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flush_on_time_or_size() {
        assert!(!should_flush_batch(1, Some(0), 49, 32, 50));
        assert!(should_flush_batch(1, Some(0), 50, 32, 50));
        assert!(should_flush_batch(32, Some(0), 1, 32, 50));
        assert!(!should_flush_batch(0, None, 1000, 32, 50));
    }
}
