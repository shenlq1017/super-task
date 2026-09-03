//! Host-level system metrics for the status bar.
//! Uses only platform APIs already available to the project: Win32 on Windows,
//! `/proc` + `statvfs` + thermal sysfs on Linux. No network, no persistence.

use serde::{Deserialize, Serialize};
use std::sync::Mutex;

/// How eagerly CPU temperature is sampled. Chosen by the user in the status bar.
///
/// On Linux every mode except [`TempMode::Off`] reads thermal sysfs directly —
/// it costs a file read, so there is nothing to throttle. On Windows the sensor
/// only comes from WMI: [`TempMode::Auto`] pays for one short PowerShell call a
/// minute, while [`TempMode::Fast`] keeps a single PowerShell process resident
/// that streams readings, so per-sample cost stays at zero new processes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TempMode {
    /// Never sample; `cpu_temp_c` is always absent.
    Off,
    /// Cheap cadence (default): live on Linux, cached ~60s on Windows.
    #[default]
    Auto,
    /// Near-real-time, backed by a resident sampler on Windows.
    Fast,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct HostMetrics {
    pub cpu_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub disk_used_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    pub cpu_temp_c: Option<f32>,
    /// False when this platform has no usable CPU sensor, so the UI can explain
    /// the dash instead of leaving the user toggling a mode that cannot work.
    pub cpu_temp_supported: bool,
    pub sampled_at_ms: u64,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn plausible_celsius(c: f32) -> Option<f32> {
    (c.is_finite() && c > 0.0 && c < 150.0).then_some(c)
}

// ---------------------------------------------------------------- Linux ----

#[cfg(target_os = "linux")]
static PREV_CPU: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn linux_cpu() -> Option<f32> {
    let stat = std::fs::read_to_string("/proc/stat").ok()?;
    let line = stat.lines().next()?;
    let nums: Vec<u64> = line
        .split_whitespace()
        .skip(1)
        .filter_map(|x| x.parse().ok())
        .collect();
    if nums.len() < 4 {
        return None;
    }
    let idle = nums[3].saturating_add(*nums.get(4).unwrap_or(&0));
    let total: u64 = nums.iter().copied().sum();
    let mut prev = PREV_CPU.lock().ok()?;
    let out = prev.and_then(|(pt, pi)| {
        let dt = total.saturating_sub(pt);
        let di = idle.saturating_sub(pi);
        (dt > 0).then(|| (100.0 * dt.saturating_sub(di) as f32 / dt as f32).clamp(0.0, 100.0))
    });
    *prev = Some((total, idle));
    out
}

#[cfg(target_os = "linux")]
fn linux_memory() -> Option<(u64, u64)> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        let mut p = line.split_whitespace();
        match p.next() {
            Some("MemTotal:") => total = p.next().and_then(|v| v.parse::<u64>().ok()),
            Some("MemAvailable:") => available = p.next().and_then(|v| v.parse::<u64>().ok()),
            _ => {}
        }
    }
    let (total, available) = (total? * 1024, available? * 1024);
    Some((total.saturating_sub(available), total))
}

#[cfg(target_os = "linux")]
fn linux_disk() -> Option<(u64, u64)> {
    let path = std::ffi::CString::new("/").ok()?;
    let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
    if unsafe { libc::statvfs(path.as_ptr(), &mut s) } != 0 {
        return None;
    }
    let frsize = s.f_frsize as u64;
    let total = (s.f_blocks as u64).saturating_mul(frsize);
    let free = (s.f_bavail as u64).saturating_mul(frsize);
    (total > 0).then(|| (total.saturating_sub(free), total))
}

#[cfg(target_os = "linux")]
fn linux_temp() -> Option<f32> {
    for i in 0..32 {
        let path = format!("/sys/class/thermal/thermal_zone{i}/temp");
        let Ok(raw) = std::fs::read_to_string(path) else {
            continue;
        };
        let Ok(v) = raw.trim().parse::<f32>() else {
            continue;
        };
        // Millidegrees on most kernels, plain degrees on a few.
        let c = if v > 1000.0 { v / 1000.0 } else { v };
        if let Some(c) = plausible_celsius(c) {
            return Some(c);
        }
    }
    None
}

#[cfg(target_os = "linux")]
pub fn sample_host_metrics(temp: TempMode) -> HostMetrics {
    let mem = linux_memory();
    let disk = linux_disk();
    // sysfs is a file read either way, so Auto and Fast behave the same here.
    // Read once and reuse it: a present reading is itself the support probe.
    let reading = linux_temp();
    let cpu_temp_c = if temp == TempMode::Off { None } else { reading };
    HostMetrics {
        cpu_percent: linux_cpu(),
        memory_used_bytes: mem.map(|m| m.0),
        memory_total_bytes: mem.map(|m| m.1),
        disk_used_bytes: disk.map(|d| d.0),
        disk_total_bytes: disk.map(|d| d.1),
        cpu_temp_c,
        cpu_temp_supported: reading.is_some(),
        sampled_at_ms: now_ms(),
    }
}

// -------------------------------------------------------------- Windows ----

#[cfg(windows)]
static PREV_CPU: Mutex<Option<(u64, u64)>> = Mutex::new(None);

#[cfg(windows)]
fn filetime_to_u64(v: windows::Win32::Foundation::FILETIME) -> u64 {
    ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64
}

#[cfg(windows)]
fn windows_cpu() -> Option<f32> {
    use windows::Win32::{Foundation::FILETIME, System::Threading::GetSystemTimes};
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    // Kernel time already includes idle time on Windows.
    let idle = filetime_to_u64(idle);
    let total = filetime_to_u64(kernel).saturating_add(filetime_to_u64(user));
    let mut prev = PREV_CPU.lock().ok()?;
    let out = prev.and_then(|(pt, pi)| {
        let dt = total.saturating_sub(pt);
        let di = idle.saturating_sub(pi);
        (dt > 0).then(|| (100.0 * dt.saturating_sub(di) as f32 / dt as f32).clamp(0.0, 100.0))
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
    (m.ullTotalPhys > 0).then(|| {
        (
            m.ullTotalPhys.saturating_sub(m.ullAvailPhys),
            m.ullTotalPhys,
        )
    })
}

#[cfg(windows)]
fn windows_disk() -> Option<(u64, u64)> {
    use windows::core::PCWSTR;
    use windows::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    // Drive holding the current working directory, e.g. `C:\`.
    let cwd = std::env::current_dir().ok()?;
    let prefix = cwd
        .components()
        .next()?
        .as_os_str()
        .to_string_lossy()
        .to_string();
    let root = if prefix.ends_with(':') {
        format!("{prefix}\\")
    } else {
        prefix
    };
    let wide: Vec<u16> = root.encode_utf16().chain(std::iter::once(0)).collect();
    let mut total: u64 = 0;
    let mut free: u64 = 0;
    unsafe {
        GetDiskFreeSpaceExW(
            PCWSTR(wide.as_ptr()),
            None,
            Some(&mut total),
            Some(&mut free),
        )
    }
    .ok()?;
    (total > 0).then(|| (total.saturating_sub(free), total))
}

/// Reads the first ACPI thermal zone, in tenths of a Kelvin.
#[cfg(windows)]
const WMI_TEMP_EXPR: &str = "(Get-CimInstance -Namespace root/wmi -ClassName MSAcpi_ThermalZoneTemperature -ErrorAction Stop | Select-Object -First 1).CurrentTemperature";

#[cfg(windows)]
fn deci_kelvin_to_celsius(raw: f32) -> Option<f32> {
    plausible_celsius(raw / 10.0 - 273.15)
}

#[cfg(windows)]
fn powershell(script: &str) -> std::process::Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let mut c = std::process::Command::new("powershell");
    c.args(["-NoProfile", "-NonInteractive", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW);
    c
}

#[cfg(windows)]
#[derive(Default)]
struct TempState {
    /// Cache for [`TempMode::Auto`]: one short PowerShell call per minute.
    slow_at: Option<std::time::Instant>,
    slow_value: Option<f32>,
    /// Consecutive failures; the sensor is declared missing after a few.
    failures: u8,
    unsupported: bool,
    /// Resident sampler for [`TempMode::Fast`].
    fast: Option<FastSampler>,
}

#[cfg(windows)]
struct FastSampler {
    child: std::process::Child,
    latest: std::sync::Arc<Mutex<Option<f32>>>,
}

#[cfg(windows)]
impl FastSampler {
    /// Spawns one PowerShell that prints a reading every ~1s, plus a thread
    /// that parses its stdout. Cost is a single long-lived process, so the
    /// status bar can poll as fast as it likes.
    fn spawn() -> Option<Self> {
        use std::io::{BufRead, BufReader};
        use std::process::Stdio;
        use std::sync::Arc;

        let script = format!(
            "$ErrorActionPreference='Stop'; while ($true) {{ try {{ $v = {WMI_TEMP_EXPR}; if ($null -ne $v) {{ Write-Output $v }} else {{ Write-Output 'na' }} }} catch {{ Write-Output 'na' }} [Console]::Out.Flush(); Start-Sleep -Milliseconds 1000 }}"
        );
        let mut child = powershell(&script)
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .stdin(Stdio::null())
            .spawn()
            .ok()?;
        let stdout = child.stdout.take()?;
        let latest: Arc<Mutex<Option<f32>>> = Arc::new(Mutex::new(None));
        let sink = Arc::clone(&latest);
        std::thread::Builder::new()
            .name("cpu-temp-sampler".into())
            .spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let value = line
                        .trim()
                        .parse::<f32>()
                        .ok()
                        .and_then(deci_kelvin_to_celsius);
                    if let Ok(mut slot) = sink.lock() {
                        *slot = value;
                    }
                }
            })
            .ok()?;
        Some(Self { child, latest })
    }

    fn read(&self) -> Option<f32> {
        self.latest.lock().ok().and_then(|v| *v)
    }

    fn stop(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[cfg(windows)]
static TEMP: Mutex<Option<TempState>> = Mutex::new(None);

#[cfg(windows)]
fn windows_temp(mode: TempMode) -> (Option<f32>, bool) {
    use std::time::{Duration, Instant};

    let Ok(mut guard) = TEMP.lock() else {
        return (None, true);
    };
    let state = guard.get_or_insert_with(TempState::default);

    // Tear the resident sampler down as soon as the user leaves Fast, so an
    // unused mode never keeps a PowerShell process alive.
    if mode != TempMode::Fast {
        if let Some(fast) = state.fast.take() {
            fast.stop();
        }
    }
    if mode == TempMode::Off {
        return (None, !state.unsupported);
    }
    if state.unsupported {
        return (None, false);
    }

    let value = match mode {
        TempMode::Fast => {
            if state.fast.is_none() {
                state.fast = FastSampler::spawn();
            }
            // A freshly spawned sampler has not printed yet; fall back to the
            // last cached reading so the number does not blink to a dash.
            state
                .fast
                .as_ref()
                .and_then(FastSampler::read)
                .or(state.slow_value)
        }
        _ => {
            let stale = match state.slow_at {
                Some(at) => at.elapsed() >= Duration::from_secs(60),
                None => true,
            };
            if !stale {
                return (state.slow_value, true);
            }
            state.slow_at = Some(Instant::now());
            powershell(WMI_TEMP_EXPR)
                .output()
                .ok()
                .filter(|o| o.status.success())
                .and_then(|o| {
                    String::from_utf8_lossy(&o.stdout)
                        .trim()
                        .parse::<f32>()
                        .ok()
                })
                .and_then(deci_kelvin_to_celsius)
        }
    };

    if value.is_some() {
        state.failures = 0;
        state.slow_value = value;
    } else {
        state.failures = state.failures.saturating_add(1);
        // Many desktops never expose MSAcpi_ThermalZoneTemperature. Give up
        // rather than retrying a query that will keep failing.
        if state.failures >= 5 {
            state.unsupported = true;
            state.slow_value = None;
            if let Some(fast) = state.fast.take() {
                fast.stop();
            }
            return (None, false);
        }
    }
    (value, true)
}

#[cfg(windows)]
pub fn sample_host_metrics(temp: TempMode) -> HostMetrics {
    let mem = windows_memory();
    let disk = windows_disk();
    let (cpu_temp_c, cpu_temp_supported) = windows_temp(temp);
    HostMetrics {
        cpu_percent: windows_cpu(),
        memory_used_bytes: mem.map(|m| m.0),
        memory_total_bytes: mem.map(|m| m.1),
        disk_used_bytes: disk.map(|d| d.0),
        disk_total_bytes: disk.map(|d| d.1),
        cpu_temp_c,
        cpu_temp_supported,
        sampled_at_ms: now_ms(),
    }
}

// --------------------------------------------------------------- Others ----

#[cfg(not(any(target_os = "linux", windows)))]
pub fn sample_host_metrics(_temp: TempMode) -> HostMetrics {
    HostMetrics {
        cpu_percent: None,
        memory_used_bytes: None,
        memory_total_bytes: None,
        disk_used_bytes: None,
        disk_total_bytes: None,
        cpu_temp_c: None,
        cpu_temp_supported: false,
        sampled_at_ms: now_ms(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_is_internally_consistent() {
        let s = sample_host_metrics(TempMode::Auto);
        if let (Some(used), Some(total)) = (s.memory_used_bytes, s.memory_total_bytes) {
            assert!(used <= total);
        }
        if let (Some(used), Some(total)) = (s.disk_used_bytes, s.disk_total_bytes) {
            assert!(used <= total);
        }
        if let Some(cpu) = s.cpu_percent {
            assert!((0.0..=100.0).contains(&cpu));
        }
    }

    #[test]
    fn off_never_reports_a_temperature() {
        assert!(sample_host_metrics(TempMode::Off).cpu_temp_c.is_none());
    }

    #[test]
    fn temp_mode_round_trips_as_lowercase() {
        assert_eq!(serde_json::to_string(&TempMode::Fast).unwrap(), "\"fast\"");
        assert_eq!(
            serde_json::from_str::<TempMode>("\"off\"").unwrap(),
            TempMode::Off
        );
        assert_eq!(TempMode::default(), TempMode::Auto);
    }

    #[test]
    fn implausible_readings_are_rejected() {
        assert_eq!(plausible_celsius(45.5), Some(45.5));
        assert_eq!(plausible_celsius(0.0), None);
        assert_eq!(plausible_celsius(f32::NAN), None);
        assert_eq!(plausible_celsius(900.0), None);
    }
}
