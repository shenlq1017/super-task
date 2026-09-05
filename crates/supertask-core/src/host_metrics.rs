//! Host-level system metrics for the status bar and the monitor page.
//! Uses only platform APIs already available to the project: Win32 on Windows,
//! `/proc` + `statvfs` + thermal sysfs + `getifaddrs` on Linux. No sockets, no
//! persistence.

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
    /// Load split over the same window as [`Self::cpu_percent`]. Windows has no
    /// nice concept, so it is always Some(0) there. Sums to ~100% with idle
    /// (Linux steal time is the leftover).
    pub cpu_user_percent: Option<f32>,
    pub cpu_system_percent: Option<f32>,
    pub cpu_nice_percent: Option<f32>,
    pub cpu_idle_percent: Option<f32>,
    pub memory_used_bytes: Option<u64>,
    pub memory_total_bytes: Option<u64>,
    pub memory_available_bytes: Option<u64>,
    /// Windows reports the commit charge vs the RAM+pagefile commit limit (the
    /// closest cheap analogue of swap; real per-pagefile usage would need
    /// undocumented NtQuerySystemInformation calls).
    pub swap_used_bytes: Option<u64>,
    pub swap_total_bytes: Option<u64>,
    pub disk_used_bytes: Option<u64>,
    pub disk_total_bytes: Option<u64>,
    pub cpu_temp_c: Option<f32>,
    /// False when this platform has no usable CPU sensor, so the UI can explain
    /// the dash instead of leaving the user toggling a mode that cannot work.
    pub cpu_temp_supported: bool,
    /// Whole-host throughput in bytes per second, normalized by the elapsed
    /// window, so pollers at different cadences stay correct. Absent on the
    /// first sample (no baseline yet).
    pub net_upload_bps: Option<f64>,
    pub net_download_bps: Option<f64>,
    /// Primary non-loopback IPv4 of this machine, for the monitor page.
    pub net_local_ip: Option<String>,
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

/// Cumulative CPU counters; the delta between two samples is one load window.
#[derive(Debug, Clone, Copy)]
struct CpuCumul {
    total: u64,
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
}

/// One load window: already-delta counters.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CpuWindow {
    total: u64,
    user: u64,
    nice: u64,
    system: u64,
    idle: u64,
}

fn cpu_window(prev: CpuCumul, cur: CpuCumul) -> Option<CpuWindow> {
    let d = |a: u64, b: u64| a.saturating_sub(b);
    let total = d(cur.total, prev.total);
    (total > 0).then(|| CpuWindow {
        total,
        user: d(cur.user, prev.user),
        nice: d(cur.nice, prev.nice),
        system: d(cur.system, prev.system),
        idle: d(cur.idle, prev.idle),
    })
}

/// (busy, user, nice, system, idle) percentages. Sums to ~100%: exact on
/// Windows, on Linux the total also counts steal/guest time.
fn split_percents(
    w: CpuWindow,
) -> (
    Option<f32>,
    Option<f32>,
    Option<f32>,
    Option<f32>,
    Option<f32>,
) {
    let p = |v: u64| Some((100.0 * v as f32 / w.total as f32).clamp(0.0, 100.0));
    (
        p(w.total.saturating_sub(w.idle)),
        p(w.user),
        p(w.nice),
        p(w.system),
        p(w.idle),
    )
}

/// Memory picture in bytes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct MemSample {
    used: u64,
    total: u64,
    available: u64,
    swap_used: u64,
    swap_total: u64,
}

/// Baseline for throughput deltas: last (sample time, download, upload).
type NetPrev = (std::time::Instant, u64, u64);

fn net_rates(
    slot: &Mutex<Option<NetPrev>>,
    now: std::time::Instant,
    download: u64,
    upload: u64,
) -> (Option<f64>, Option<f64>) {
    let Ok(mut prev) = slot.lock() else {
        return (None, None);
    };
    let out = match *prev {
        Some((at, pd, pu)) => {
            let dt = now.duration_since(at).as_secs_f64();
            if dt > 0.0 {
                (
                    Some(download.saturating_sub(pd) as f64 / dt),
                    Some(upload.saturating_sub(pu) as f64 / dt),
                )
            } else {
                (None, None)
            }
        }
        None => (None, None),
    };
    *prev = Some((now, download, upload));
    out
}

// ---------------------------------------------------------------- Linux ----

#[cfg(target_os = "linux")]
static PREV_CPU: Mutex<Option<CpuCumul>> = Mutex::new(None);

#[cfg(target_os = "linux")]
fn linux_cpu() -> Option<CpuWindow> {
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
    let g = |i: usize| nums.get(i).copied().unwrap_or(0);
    // Columns: user nice system idle iowait irq softirq steal ... The total
    // keeps every column; irq+softirq fold into system and iowait into idle
    // (same grouping the aggregate busy figure always used).
    let cur = CpuCumul {
        total: nums.iter().copied().sum(),
        user: nums[0],
        nice: g(1),
        system: g(2) + g(5) + g(6),
        idle: g(3) + g(4),
    };
    let mut prev = PREV_CPU.lock().ok()?;
    let out = prev.and_then(|p| cpu_window(p, cur));
    *prev = Some(cur);
    out
}

#[cfg(target_os = "linux")]
fn linux_memory() -> Option<MemSample> {
    let text = std::fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    let mut swap_total = 0u64;
    let mut swap_free = 0u64;
    for line in text.lines() {
        let mut p = line.split_whitespace();
        match p.next() {
            Some("MemTotal:") => total = p.next().and_then(|v| v.parse::<u64>().ok()),
            Some("MemAvailable:") => available = p.next().and_then(|v| v.parse::<u64>().ok()),
            Some("SwapTotal:") => {
                swap_total = p.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
            }
            Some("SwapFree:") => {
                swap_free = p.next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0)
            }
            _ => {}
        }
    }
    // meminfo counts in KiB.
    let (total, available) = (total? * 1024, available? * 1024);
    Some(MemSample {
        used: total.saturating_sub(available),
        total,
        available,
        swap_used: swap_total.saturating_sub(swap_free) * 1024,
        swap_total: swap_total * 1024,
    })
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
static PREV_NET: Mutex<Option<NetPrev>> = Mutex::new(None);

/// Sums receive/transmit bytes over all physical interfaces.
#[cfg(target_os = "linux")]
fn linux_net() -> (Option<f64>, Option<f64>) {
    let Ok(text) = std::fs::read_to_string("/proc/net/dev") else {
        return (None, None);
    };
    let mut download = 0u64;
    let mut upload = 0u64;
    for line in text.lines().skip(2) {
        let Some((name, rest)) = line.split_once(':') else {
            continue;
        };
        if name.trim() == "lo" {
            continue;
        }
        let fields: Vec<u64> = rest
            .split_whitespace()
            .filter_map(|x| x.parse().ok())
            .collect();
        if fields.len() < 9 {
            continue;
        }
        download += fields[0];
        upload += fields[8];
    }
    net_rates(&PREV_NET, std::time::Instant::now(), download, upload)
}

/// First non-loopback IPv4, e.g. `192.168.1.10`.
#[cfg(target_os = "linux")]
fn linux_local_ip() -> Option<String> {
    unsafe {
        let mut head: *mut libc::ifaddrs = std::ptr::null_mut();
        if libc::getifaddrs(&mut head) != 0 {
            return None;
        }
        let mut found = None;
        let mut p = head;
        while !p.is_null() {
            let a = &*p;
            p = a.ifa_next;
            let Some(sa) = a.ifa_addr else { continue };
            if (*sa).sa_family as i32 != libc::AF_INET {
                continue;
            }
            let name = std::ffi::CStr::from_ptr(a.ifa_name).to_string_lossy();
            if name == "lo" {
                continue;
            }
            let sin = &*(sa as *const libc::sockaddr_in);
            // s_addr is stored in network byte order; from_be+to_be_bytes
            // restores the on-wire octet order on either host endianness.
            let b = u32::from_be(sin.sin_addr.s_addr).to_be_bytes();
            found = Some(format!("{}.{}.{}.{}", b[0], b[1], b[2], b[3]));
            break;
        }
        libc::freeifaddrs(head);
        found
    }
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
    let (cpu_percent, cpu_user_percent, cpu_nice_percent, cpu_system_percent, cpu_idle_percent) =
        linux_cpu().map_or((None, None, None, None, None), split_percents);
    let (net_download_bps, net_upload_bps) = linux_net();
    HostMetrics {
        cpu_percent,
        cpu_user_percent,
        cpu_system_percent,
        cpu_nice_percent,
        cpu_idle_percent,
        memory_used_bytes: mem.map(|m| m.used),
        memory_total_bytes: mem.map(|m| m.total),
        memory_available_bytes: mem.map(|m| m.available),
        swap_used_bytes: mem.map(|m| m.swap_used),
        swap_total_bytes: mem.map(|m| m.swap_total),
        disk_used_bytes: disk.map(|d| d.0),
        disk_total_bytes: disk.map(|d| d.1),
        cpu_temp_c,
        cpu_temp_supported: reading.is_some(),
        net_download_bps,
        net_upload_bps,
        net_local_ip: linux_local_ip(),
        sampled_at_ms: now_ms(),
    }
}

// -------------------------------------------------------------- Windows ----

#[cfg(windows)]
static PREV_CPU: Mutex<Option<CpuCumul>> = Mutex::new(None);

#[cfg(windows)]
fn filetime_to_u64(v: windows::Win32::Foundation::FILETIME) -> u64 {
    ((v.dwHighDateTime as u64) << 32) | v.dwLowDateTime as u64
}

#[cfg(windows)]
fn windows_cpu() -> Option<CpuWindow> {
    use windows::Win32::{Foundation::FILETIME, System::Threading::GetSystemTimes};
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) }.ok()?;
    // Kernel time already includes idle time on Windows; there is no nice time.
    let idle = filetime_to_u64(idle);
    let kernel = filetime_to_u64(kernel);
    let user = filetime_to_u64(user);
    let cur = CpuCumul {
        total: kernel.saturating_add(user),
        user,
        nice: 0,
        system: kernel.saturating_sub(idle),
        idle,
    };
    let mut prev = PREV_CPU.lock().ok()?;
    let out = prev.and_then(|p| cpu_window(p, cur));
    *prev = Some(cur);
    out
}

#[cfg(windows)]
fn windows_memory() -> Option<MemSample> {
    use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut m = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    unsafe { GlobalMemoryStatusEx(&mut m) }.ok()?;
    (m.ullTotalPhys > 0).then(|| MemSample {
        used: m.ullTotalPhys.saturating_sub(m.ullAvailPhys),
        total: m.ullTotalPhys,
        available: m.ullAvailPhys,
        swap_used: m.ullTotalPageFile.saturating_sub(m.ullAvailPageFile),
        swap_total: m.ullTotalPageFile,
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

/// IANA ifType 24: software loopback. Traffic to localhost is not "network".
#[cfg(windows)]
const IF_TYPE_LOOPBACK: u32 = 24;

#[cfg(windows)]
static PREV_NET: Mutex<Option<NetPrev>> = Mutex::new(None);

/// Sums octets over all non-loopback interfaces via GetIfTable2.
#[cfg(windows)]
fn windows_net() -> (Option<f64>, Option<f64>) {
    use windows::Win32::NetworkManagement::IpHelper::{FreeMibTable, GetIfTable2, MIB_IF_TABLE2};

    let mut table: *mut MIB_IF_TABLE2 = std::ptr::null_mut();
    let (mut download, mut upload) = (0u64, 0u64);
    unsafe {
        if GetIfTable2(&mut table).is_err() || table.is_null() {
            return (None, None);
        }
        let t = &*table;
        let rows = std::slice::from_raw_parts(t.Table.as_ptr(), t.NumEntries as usize);
        for row in rows {
            if row.Type == IF_TYPE_LOOPBACK {
                continue;
            }
            download += row.InOctets;
            upload += row.OutOctets;
        }
        FreeMibTable(table.cast());
    }
    net_rates(&PREV_NET, std::time::Instant::now(), download, upload)
}

/// IPv4 of an adapter on the default route (has a gateway), else the first
/// connected non-loopback IPv4.
#[cfg(windows)]
fn windows_local_ip() -> Option<String> {
    use windows::Win32::Foundation::ERROR_BUFFER_OVERFLOW;
    use windows::Win32::NetworkManagement::IpHelper::{
        GetAdaptersAddresses, GAA_FLAG_INCLUDE_GATEWAYS, GAA_FLAG_SKIP_ANYCAST,
        GAA_FLAG_SKIP_DNS_SERVER, GAA_FLAG_SKIP_MULTICAST, GET_ADAPTERS_ADDRESSES_FLAGS,
        IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_UNICAST_ADDRESS_LH,
    };
    use windows::Win32::NetworkManagement::Ndis::IfOperStatusUp;
    use windows::Win32::Networking::WinSock::{AF_INET, AF_UNSPEC, SOCKADDR_IN};

    let flags = GET_ADAPTERS_ADDRESSES_FLAGS(
        GAA_FLAG_SKIP_ANYCAST.0
            | GAA_FLAG_SKIP_MULTICAST.0
            | GAA_FLAG_SKIP_DNS_SERVER.0
            | GAA_FLAG_INCLUDE_GATEWAYS.0,
    );
    let mut len = 32usize; // u64 slots; grows on overflow. Plenty for a desktop.
    let buf = loop {
        let mut buf = vec![0u64; len];
        let mut size = (buf.len() * std::mem::size_of::<u64>()) as u32;
        let rc = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr() as *mut IP_ADAPTER_ADDRESSES_LH),
                &mut size,
            )
        };
        if rc == ERROR_BUFFER_OVERFLOW.0 {
            len *= 2;
            if len > 2048 {
                return None;
            }
            continue;
        }
        if rc != 0 {
            return None;
        }
        break buf;
    };

    let mut fallback: Option<String> = None;
    unsafe {
        let mut p = buf.as_ptr() as *const IP_ADAPTER_ADDRESSES_LH;
        while !p.is_null() {
            let a = &*p;
            p = a.Next;
            if a.OperStatus != IfOperStatusUp || a.IfType == IF_TYPE_LOOPBACK {
                continue;
            }
            let mut ip = None;
            let mut ua: *mut IP_ADAPTER_UNICAST_ADDRESS_LH = a.FirstUnicastAddress;
            while !ua.is_null() {
                let sa = (*ua).Address.lpSockaddr;
                ua = (*ua).Next;
                if sa.is_null() || (*sa).sa_family != AF_INET {
                    continue;
                }
                let sin = &*(sa as *const SOCKADDR_IN);
                let b = &sin.sin_addr.S_un.S_un_b;
                ip = Some(format!("{}.{}.{}.{}", b.s_b1, b.s_b2, b.s_b3, b.s_b4));
                break;
            }
            if let Some(s) = ip {
                if !a.FirstGatewayAddress.is_null() {
                    return Some(s);
                }
                if fallback.is_none() {
                    fallback = Some(s);
                }
            }
        }
    }
    fallback
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
    let (cpu_percent, cpu_user_percent, cpu_nice_percent, cpu_system_percent, cpu_idle_percent) =
        windows_cpu().map_or((None, None, None, None, None), split_percents);
    let (net_download_bps, net_upload_bps) = windows_net();
    HostMetrics {
        cpu_percent,
        cpu_user_percent,
        cpu_system_percent,
        cpu_nice_percent,
        cpu_idle_percent,
        memory_used_bytes: mem.map(|m| m.used),
        memory_total_bytes: mem.map(|m| m.total),
        memory_available_bytes: mem.map(|m| m.available),
        swap_used_bytes: mem.map(|m| m.swap_used),
        swap_total_bytes: mem.map(|m| m.swap_total),
        disk_used_bytes: disk.map(|d| d.0),
        disk_total_bytes: disk.map(|d| d.1),
        cpu_temp_c,
        cpu_temp_supported,
        net_download_bps,
        net_upload_bps,
        net_local_ip: windows_local_ip(),
        sampled_at_ms: now_ms(),
    }
}

// --------------------------------------------------------------- Others ----

#[cfg(not(any(target_os = "linux", windows)))]
pub fn sample_host_metrics(_temp: TempMode) -> HostMetrics {
    HostMetrics {
        cpu_percent: None,
        cpu_user_percent: None,
        cpu_system_percent: None,
        cpu_nice_percent: None,
        cpu_idle_percent: None,
        memory_used_bytes: None,
        memory_total_bytes: None,
        memory_available_bytes: None,
        swap_used_bytes: None,
        swap_total_bytes: None,
        disk_used_bytes: None,
        disk_total_bytes: None,
        cpu_temp_c: None,
        cpu_temp_supported: false,
        net_upload_bps: None,
        net_download_bps: None,
        net_local_ip: None,
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
        if let (Some(used), Some(total)) = (s.swap_used_bytes, s.swap_total_bytes) {
            assert!(used <= total);
        }
        if let (Some(used), Some(total)) = (s.disk_used_bytes, s.disk_total_bytes) {
            assert!(used <= total);
        }
        if let Some(cpu) = s.cpu_percent {
            assert!((0.0..=100.0).contains(&cpu));
        }
        for v in [
            s.cpu_user_percent,
            s.cpu_nice_percent,
            s.cpu_system_percent,
            s.cpu_idle_percent,
        ] {
            if let Some(v) = v {
                assert!((0.0..=100.0).contains(&v));
            }
        }
        if let (Some(down), Some(up)) = (s.net_download_bps, s.net_upload_bps) {
            assert!(down >= 0.0 && up >= 0.0);
        }
        if let Some(ip) = &s.net_local_ip {
            assert!(ip.contains('.'), "ipv4-looking: {ip}");
        }
    }

    #[test]
    fn split_percents_sum_to_about_100() {
        let w = CpuWindow {
            total: 1000,
            user: 250,
            nice: 50,
            system: 200,
            idle: 500,
        };
        let (busy, user, nice, system, idle) = split_percents(w);
        assert_eq!(busy, Some(50.0));
        assert_eq!(user, Some(25.0));
        assert_eq!(nice, Some(5.0));
        assert_eq!(system, Some(20.0));
        assert_eq!(idle, Some(50.0));
        let sum = user.unwrap() + nice.unwrap() + system.unwrap() + idle.unwrap();
        assert!((sum - 100.0).abs() < 0.01);
    }

    #[test]
    fn cpu_window_needs_a_positive_delta() {
        let c = CpuCumul {
            total: 100,
            user: 1,
            nice: 2,
            system: 3,
            idle: 94,
        };
        assert!(cpu_window(c, c).is_none());
        let later = CpuCumul {
            total: 200,
            user: 51,
            nice: 2,
            system: 53,
            idle: 144,
        };
        let w = cpu_window(c, later).unwrap();
        assert_eq!(w.total, 100);
        assert_eq!(w.user, 50);
        assert_eq!(w.idle, 50);
    }

    #[test]
    fn net_rates_normalize_by_elapsed_time() {
        static NET: Mutex<Option<NetPrev>> = Mutex::new(None);
        let t0 = std::time::Instant::now();
        // First sample primes the baseline: no rate yet.
        assert_eq!(net_rates(&NET, t0, 1000, 500), (None, None));
        // 2s later, 2000 B received and 500 B sent → 1000 / 250 per second.
        let (down, up) = net_rates(&NET, t0 + std::time::Duration::from_secs(2), 3000, 1000);
        assert_eq!(down, Some(1000.0));
        assert_eq!(up, Some(250.0));
        // Counter reset (interface restart): rates stay sane instead of negative.
        let (down, up) = net_rates(&NET, t0 + std::time::Duration::from_secs(3), 100, 50);
        assert_eq!(down, Some(0.0));
        assert_eq!(up, Some(0.0));
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
