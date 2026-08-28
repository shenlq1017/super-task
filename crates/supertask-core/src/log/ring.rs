use serde::{Deserialize, Serialize};

use crate::ipc::{LogSource, LogStream, MAX_LOG_LINE_BYTES};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LogLine {
    pub seq: u64,
    pub source: LogSource,
    pub stream: LogStream,
    pub ts_ms: u64,
    pub text: String,
}

pub fn truncate_line(mut text: String) -> String {
    if text.len() <= MAX_LOG_LINE_BYTES {
        return text;
    }
    let mut end = MAX_LOG_LINE_BYTES;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
    text.push('…');
    text
}

/// Per-source ring + workspace-wide seq.
pub struct LogHub {
    cap: usize,
    next_seq: u64,
    buf: std::collections::VecDeque<LogLine>,
}

impl LogHub {
    pub fn new(cap: usize) -> Self {
        Self {
            cap: cap.max(1),
            next_seq: 1,
            buf: std::collections::VecDeque::new(),
        }
    }

    pub fn push(&mut self, mut line: LogLine) -> LogLine {
        line.text = truncate_line(line.text);
        line.seq = self.next_seq;
        self.next_seq += 1;
        if self.buf.len() >= self.cap {
            self.buf.pop_front();
        }
        self.buf.push_back(line.clone());
        line
    }

    pub fn snapshot(&self, source: Option<&LogSource>, limit: usize) -> (Vec<LogLine>, u64) {
        let items: Vec<LogLine> = self
            .buf
            .iter()
            .filter(|l| source.map(|s| &l.source == s).unwrap_or(true))
            .cloned()
            .rev()
            .take(limit)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect();
        (items, self.next_seq)
    }

    pub fn since(&self, seq: u64) -> Vec<LogLine> {
        self.buf.iter().filter(|l| l.seq >= seq).cloned().collect()
    }

    pub fn clear_source(&mut self, source: &LogSource) {
        self.buf.retain(|l| &l.source != source);
    }

    pub fn next_seq(&self) -> u64 {
        self.next_seq
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ipc::LogSourceKind;

    fn src() -> LogSource {
        LogSource {
            kind: LogSourceKind::Service,
            id: "api".into(),
        }
    }

    #[test]
    fn ring_drops_oldest() {
        let mut h = LogHub::new(2);
        let mk = |t: &str| LogLine {
            seq: 0,
            source: src(),
            stream: LogStream::Stdout,
            ts_ms: 0,
            text: t.into(),
        };
        h.push(mk("a"));
        h.push(mk("b"));
        h.push(mk("c"));
        let (items, _) = h.snapshot(None, 10);
        assert_eq!(
            items.iter().map(|l| l.text.as_str()).collect::<Vec<_>>(),
            vec!["b", "c"]
        );
        assert_eq!(items[0].seq, 2);
        assert_eq!(items[1].seq, 3);
    }

    #[test]
    fn truncate_utf8() {
        let s = "你".repeat(5000);
        let t = truncate_line(s);
        assert!(t.ends_with('…'));
        assert!(t.len() <= MAX_LOG_LINE_BYTES + 3);
    }
}
