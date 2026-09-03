//! Host-level system metrics for the status bar (CPU / memory / disk / CPU temp).
//! Separate from workspace Job-tree [`crate::metrics`] samples.

use serde::Serialize;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use sysinfo::{CpuRefreshKind, Disks, MemoryRefreshKind, RefreshKind, System};

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostMetrics {
    /// Aggregate CPU usage percent (0–100-ish across cores average).
    pub cpu_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    /// Sum of local disks (excludes remount/network when possible).
    pub disk_used_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    /// CPU package / first available thermal sensor in °C, if the OS exposes it.
    pub cpu_temp_c: Option<f32>,
    pub sampled_at_ms: u64,
}

struct Cache {
    system: System,
    disks: Disks,
    last: Instant,
    last_snapshot: Option<HostMetrics>,
}

impl Cache {
    fn new() -> Self {
        let mut system = System::new_with_specifics(
            RefreshKind::nothing()
                .with_cpu(CpuRefreshKind::everything())
                .with_memory(MemoryRefreshKind::everything()),
        );
        // First CPU refresh establishes a baseline; second call yields a real %.
        system.refresh_cpu_all();
        let disks = Disks::new_with_refreshed_list();
        Self {
            system,
            disks,
            last: Instant::now() - Duration::from_secs(10),
            last_snapshot: None,
        }
    }
}

static CACHE: Mutex<Option<Cache>> = Mutex::new(None);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn cpu_temp_c() -> Option<f32> {
    // Components API: pick the first CPU-ish sensor. Not available on all hosts.
    let components = sysinfo::Components::new_with_refreshed_list();
    let mut best: Option<f32> = None;
    for c in components.iter() {
        let label = c.label().to_ascii_lowercase();
        let Some(t) = c.temperature() else {
            continue;
        };
        if !t.is_finite() || t <= 0.0 {
            continue;
        }
        let is_cpu = label.contains("cpu")
            || label.contains("tctl")
            || label.contains("package")
            || label.contains("core");
        if is_cpu {
            return Some(t);
        }
        if best.is_none() {
            best = Some(t);
        }
    }
    best
}

/// Sample host metrics. Throttled to ~1.5s so status-bar polling stays cheap.
pub fn sample_host_metrics() -> HostMetrics {
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    let cache = guard.get_or_insert_with(Cache::new);

    if let Some(prev) = &cache.last_snapshot {
        if cache.last.elapsed() < Duration::from_millis(1500) {
            return prev.clone();
        }
    }

    // Two close CPU refreshes improve accuracy on cold start.
    cache.system.refresh_cpu_all();
    std::thread::sleep(Duration::from_millis(120));
    cache.system.refresh_cpu_all();
    cache.system.refresh_memory();
    cache.disks.refresh(true);

    let cpu_percent = {
        let cpus = cache.system.cpus();
        if cpus.is_empty() {
            None
        } else {
            let sum: f32 = cpus.iter().map(|c| c.cpu_usage()).sum();
            Some((sum / cpus.len() as f32).clamp(0.0, 100.0))
        }
    };

    let memory_total = cache.system.total_memory();
    let memory_used = cache.system.used_memory();
    let (memory_used_bytes, memory_total_bytes) = if memory_total > 0 {
        (Some(memory_used), Some(memory_total))
    } else {
        (None, None)
    };

    let mut disk_total = 0u64;
    let mut disk_available = 0u64;
    for d in cache.disks.list() {
        // Skip tiny / virtual mounts.
        let total = d.total_space();
        if total < 1_000_000_000 {
            continue;
        }
        disk_total = disk_total.saturating_add(total);
        disk_available = disk_available.saturating_add(d.available_space());
    }
    let (disk_used_bytes, disk_total_bytes) = if disk_total > 0 {
        (
            Some(disk_total.saturating_sub(disk_available)),
            Some(disk_total),
        )
    } else {
        (None, None)
    };

    let snap = HostMetrics {
        cpu_percent,
        memory_used_bytes,
        memory_total_bytes,
        disk_used_bytes,
        disk_total_bytes,
        cpu_temp_c: cpu_temp_c(),
        sampled_at_ms: now_ms(),
    };
    cache.last = Instant::now();
    cache.last_snapshot = Some(snap.clone());
    snap
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_returns_memory_or_none() {
        let s = sample_host_metrics();
        // Memory should almost always be present; don't hard-fail on exotic CI.
        if let (Some(u), Some(t)) = (s.memory_used_bytes, s.memory_total_bytes) {
            assert!(t > 0);
            assert!(u <= t);
        }
    }
}
