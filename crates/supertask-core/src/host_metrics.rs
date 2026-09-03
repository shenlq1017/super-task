//! Host-level system metrics for the status bar.
//! Uses only platform APIs already available to the project: Win32 on Windows,
//! `/proc` + `statvfs` + thermal sysfs on Linux. No network or persistence.

use serde::Serialize;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostMetrics {
    pub cpu_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub disk_used_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    pub cpu_temp_c: Option<f32>,
    pub sampled_at_ms: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(target_os = "linux")]
static PREV_CPU: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn linux_cpu() -> Option<f32> {
    let line = std::fs::read_to_string("/proc/stat").ok()?.lines().next()?.to_string();
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if nums.len() < 4 { return None; }
    let idle = nums[3].saturating_add(*nums.get(4).unwrap_or(&0));
    let total: u64 = nums.iter().copied().sum();
    let mut prev = PREV_CPU.lock().ok()?;
    let out = prev.and_then(|(pt, pi)| {
        let dt = total.saturating_sub(pt);
        let di = idle.saturating_sub(pi);
        (dt > 0).then(|| (100.0 * (dt.saturating_sub(di)) as f32 / dt as f32).clamp(0.0, 100.0))
    });
    *prev = Some((total, idle));
    out
}

#[cfg(target_os = "linux")]
fn linux_memory() -> (Option<u64>, Option<u64>) {
    let Ok(text) = std::fs::read_to_string("/proc/meminfo") else { return (None, None) };
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        let mut p = line.split_whitespace();
        match p.next() {
            Some("MemTotal:") => total = p.next().and_then(|v| v.parse::<u64>().ok()).map(|v| v * 1024),
            Some("MemAvailable:") => available = p.next().and_then(|v| v.parse::<u64>().ok()).map(|v| v * 1024),
            _ => {}
        }
    }
    (total.zip(available).map(|(t, a)| t.saturating_sub(a)), total)
}

#[cfg(target_os = "linux")]
fn linux_disk() -> (Option<u64>, Option<u64>) {
    use std::ffi::CString;
    let path = CString::new("/").ok();
    let Some(path) = path else { return (None, None) };
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 { return (None, None); }
    let total = s.f_blocks.saturating_mul(s.f_frsize);
    let free = s.f_bavail.saturating_mul(s.f_frsize);
    (Some(total.saturating_sub(free)), Some(total))
}

#[cfg(target_os = "linux")]
fn linux_temp() -> Option<f32> {
    for i in 0..32 {
        let p = format!("/sys/class/thermal/thermal_zone{}/temp", i);
        if let Ok(s) = std::fs::read_to_string(p) {
            if let Ok(raw) = s.trim().parse::<f32>() {
                let c = if raw > 1000.0 { raw / 1000.0 } else { raw };
                if c.is_finite() && c > 0.0 && c < 150.0 { return Some(c); }
            }
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub fn sample_host_metrics() -> HostMetrics {
    let (memory_used_bytes, memory_total_bytes) = linux_memory();
    let (disk_used_bytes, disk_total_bytes) = linux_disk();
    HostMetrics {
        cpu_percent: linux_cpu(), memory_used_bytes, memory_total_bytes,
        disk_used_bytes, disk_total_bytes, cpu_temp_c: linux_temp(), sampled_at_ms: now_ms(),
    }
}

#[cfg(windows)]
static PREV_CPU: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[cfg(windows)]
fn ft(v: windows::Win32::Foundation::FILETIME) -> u64 {
    ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64
}

#[cfg(windows)]
fn windows_cpu() -> Option<f32> {
    use windows::Win32::{Foundation::FILETIME, System::Threading::GetSystemTimes};
    let (mut idle, mut kernel, mut user) = (FILETIME::default(), FILETIME::default(), FILETIME::default());
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)).ok()?; }
    let idle = ft(idle);
    let total = ft(kernel).saturating_add(ft(user));
    let mut prev = PREV_CPU.lock().ok()?;
    let out = prev.and_then(|(pt, pi)| {
        let dt = total.saturating_sub(pt);
        let di = idle.saturating_sub(pi);
        (dt > 0).then(|| (100.0 * (dt.saturating_sub(di)) as f32 / dt as f32).clamp(0.0, 100.0))
    });
    *prev = Some((total, idle));
    out
}

#[cfg(windows)]
fn windows_memory() -> Option<(u64, u64)> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    if m.ullTotalPhys == 0 {
        return None;
    }
    Some((m.ullTotalPhys.saturating_sub(m.ullAvailPhys), m.ullTotalPhys))
}

#[cfg(windows)]
fn windows_disk() -> Option<(u64, u64)> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    // Drive that holds the current working directory, e.g. "C:\\".
    let cwd = std::env::current_dir().ok()?;
    let prefix = cwd.components().next()?.as_os_str().to_string_lossy().to_string();
    let root = if prefix.ends_with(':') {
        format!("{}\\", prefix)
    } else {
        prefix
    };
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut total: u64 = 0;
    let mut free: u64 = 0;
    unsafe { GetDiskFreeSpaceExW(PCWSTR(wide.as_ptr()), None, Some(&mut total), Some(&mut free)) }
        .ok()?;
    if total == 0 {
        return None;
    }
    Some((total.saturating_sub(free), total))
}

#[cfg(windows)]
struct TempCache {
    last: Option<std::time::Instant>,
    value: Option<f32>,
    failures: u8,
}

#[cfg(windows)]
static TEMP_CACHE: Mutex<TempCache> = Mutex::new(TempCache {
    last: None,
    value: None,
    failures: 0,
});

/// CPU temperature via WMI (`MSAcpi_ThermalZoneTemperature`, tenths of Kelvin).
/// Spawning PowerShell is expensive, so this is cached for a minute and gives
/// up entirely after two failures — many desktops never expose the sensor.
#[cfg(windows)]
fn windows_temp() -> Option<f32> {
    use std::os::windows::process::CommandExt;
    use std::process::Command;
    use std::time::{Duration, Instant};

    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let mut cache = TEMP_CACHE.lock().ok()?;
    if cache.failures >= 2 {
        return None;
    }
    if let Some(last) = cache.last {
        if last.elapsed() < Duration::from_secs(60) {
            return cache.value;
        }
    }
    cache.last = Some(Instant::now());

    let out = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop | Select-Object -First 1).CurrentTemperature",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();

    let parsed = out.ok().and_then(|o| {
        if !o.status.success() {
            return None;
        }
        let raw: f32 = String::from_utf8_lossy(&o.stdout).trim().parse().ok()?;
        // Tenths of Kelvin -> Celsius.
        let c = raw / 10.0 - 273.15;
        (c.is_finite() && c > 0.0 && c < 150.0).then_some(c)
    });

    if parsed.is_none() {
        cache.failures = cache.failures.saturating_add(1);
    } else {
        cache.failures = 0;
    }
    cache.value = parsed;
    parsed
}

#[cfg(windows)]
pub fn sample_host_metrics() -> HostMetrics {
    let mem = windows_memory();
    let disk = windows_disk();
    HostMetrics {
        cpu_percent: windows_cpu(),
        memory_used_bytes: mem.map(|m| m.0),
        memory_total_bytes: mem.map(|m| m.1),
        disk_used_bytes: disk.map(|d| d.0),
        disk_total_bytes: disk.map(|d| d.1),
        cpu_temp_c: windows_temp(),
        sampled_at_ms: now_ms(),
    }
}

#[cfg(not(any(target_os = "linux", windows)))]
pub fn sample_host_metrics() -> HostMetrics {
    HostMetrics { cpu_percent: None, memory_used_bytes: None, memory_total_bytes: None,
        disk_used_bytes: None, disk_total_bytes: None, cpu_temp_c: None, sampled_at_ms: now_ms() }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sample_is_sane() {
        let s = sample_host_metrics();
        if let (Some(u), Some(t)) = (s.memory_used_bytes, s.memory_total_bytes) { assert!(u <= t); }
        if let (Some(u), Some(t)) = (s.disk_used_bytes, s.disk_total_bytes) { assert!(u <= t); }
    }
}
