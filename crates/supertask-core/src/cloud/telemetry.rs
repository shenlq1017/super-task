//! 遥测（v2.0 规格 §9，默认关）。
//! - 事件白名单全集：app_start / app_stop / feature_open(feature_id) / service_start(kind)；
//!   无路径、无名称、无 yaml 内容；
//! - 批量：24h 或退出时一次；
//! - **关闭 = 完全 no-op（零网络请求，有单测断言）**。

use serde::{Deserialize, Serialize};

/// 白名单事件（全集，spec §9——枚举外不存在）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TelemetryEvent {
    AppStart,
    AppStop,
    FeatureOpen { feature_id: String },
    ServiceStart { kind: String },
}

/// 上报缓冲；`enabled=false` 时所有记录调用直接丢弃，flush 变 no-op。
pub struct TelemetryBuffer {
    enabled: bool,
    events: Vec<TelemetryEvent>,
    /// 上次 flush 时间戳（ms）；24h 节奏由调用方按此判断。
    last_flush_ms: u64,
}

pub const FLUSH_INTERVAL_MS: u64 = 24 * 60 * 60 * 1000;

impl TelemetryBuffer {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled,
            events: Vec::new(),
            last_flush_ms: 0,
        }
    }

    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.events.clear();
        }
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    pub fn record(&mut self, event: TelemetryEvent) {
        if self.enabled {
            self.events.push(event);
        }
    }

    pub fn should_flush(&self, now_ms: u64) -> bool {
        self.enabled
            && !self.events.is_empty()
            && now_ms.saturating_sub(self.last_flush_ms) >= FLUSH_INTERVAL_MS
    }

    /// 上报并清空；disabled 时 never 调用 provider（零请求断言点）。
    pub fn flush(
        &mut self,
        now_ms: u64,
        provider: Option<&dyn crate::cloud::CloudProvider>,
        token: &str,
    ) -> crate::error::Result<usize> {
        if !self.enabled || self.events.is_empty() {
            return Ok(0);
        }
        self.last_flush_ms = now_ms;
        let n = self.events.len();
        if let Some(p) = provider {
            let body = serde_json::json!({ "events": &self.events }).to_string();
            p.telemetry_batch(token, &body)?;
        }
        self.events.clear();
        Ok(n)
    }

    pub fn pending(&self) -> usize {
        self.events.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::fake::FakeCloudProvider;

    #[test]
    fn disabled_is_complete_noop_zero_requests() {
        let fake = FakeCloudProvider::new();
        let mut buf = TelemetryBuffer::new(false);
        buf.record(TelemetryEvent::AppStart);
        buf.record(TelemetryEvent::ServiceStart {
            kind: "python".into(),
        });
        assert_eq!(buf.pending(), 0);
        assert!(!buf.should_flush(999_999_999));
        // flush 不触达 provider
        buf.flush(1, Some(&fake), "t").unwrap();
        assert!(fake.telemetry_requests().is_empty(), "关闭时必须零网络请求");
    }

    #[test]
    fn enabled_buffers_and_flushes_whitelist_only() {
        let fake = FakeCloudProvider::new();
        let mut buf = TelemetryBuffer::new(true);
        buf.record(TelemetryEvent::FeatureOpen {
            feature_id: "run".into(),
        });
        buf.record(TelemetryEvent::AppStart);
        assert_eq!(buf.pending(), 2);
        assert!(!buf.should_flush(1000), "24h 内不批量上报");
        buf.flush(FLUSH_INTERVAL_MS, Some(&fake), "t").unwrap();
        let reqs = fake.telemetry_requests();
        assert_eq!(reqs.len(), 1);
        // 事件体只含白名单字段，无路径/内容
        assert!(
            reqs[0].contains("\"event\":\"feature_open\"")
                || reqs[0].contains("\"event\":\"app_start\"")
        );
        assert!(!reqs[0].contains("C:\\"));
        // 清空后再次 flush 零请求
        buf.flush(FLUSH_INTERVAL_MS * 2, Some(&fake), "t").unwrap();
        assert_eq!(fake.telemetry_requests().len(), 1);
    }
}
