//! 系统级服务发现：LISTEN 端口（IPv4 + IPv6）+ 进程名筛选。
//!
//! 端口枚举走 netstat2（GetExtendedTcpTable 的成熟封装，替代手解 TCP 表——
//! SDK 表结构带 pack 打包，手写偏移在 x64 上必错）。进程名走 Toolhelp，
//! cwd / cmdline 走 NtQueryInformationProcess + PEB。CPU 走 GetProcessTimes
//! 差分采样（首个观察周期为 None），内存走 GetProcessMemoryInfo 工作集。
//! 均同步调用，<50ms。
//!
//! 错误策略：端口表 / 进程快照读取失败显式返回 Err（DISCOVER），不静默空列表。

use std::collections::{BTreeMap, HashMap};

use serde::Serialize;

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Serialize)]
pub struct ForeignService {
    pub pid: u32,
    /// javaw.exe / node.exe / python.exe …
    pub name: String,
    /// 运行时归类：java / node / python / deno / bun / other
    pub kind: String,
    /// 该进程所有 LISTEN 端口（升序，含 IPv4 与 IPv6）
    pub ports: Vec<u16>,
    /// 进程当前工作目录；读取失败为 None
    pub cwd: Option<String>,
    /// 完整命令行；读取失败为 None
    pub cmd_line: Option<String>,
    /// CPU 占用百分比（相对整机逻辑核，0~100*N）。首次采样无差值或读取失败为 None
    pub cpu_percent: Option<f32>,
    /// 物理内存占用（工作集，字节）。读取失败为 None
    pub memory_bytes: Option<u64>,
}

/// 关心的运行时进程名前缀（大小写不敏感匹配文件名开头）。
pub const INTERESTING_PREFIXES: &[&str] = &["java", "node", "python", "deno", "bun"];

/// 按可执行文件名归类运行时；白名单外归 `other`（UI 默认折叠展示）。
pub fn classify_process(name: &str) -> &'static str {
    let n = name.to_lowercase();
    for p in INTERESTING_PREFIXES {
        if n.starts_with(p) {
            return p;
        }
    }
    "other"
}

/// 枚举系统中「正在监听端口」的进程（含白名单外的 other）。
pub fn discover_services() -> Result<Vec<ForeignService>> {
    let listeners = listen_ports_by_pid()
        .map_err(|e| Error::new(ErrorCode::Discover, format!("读取系统 LISTEN 端口表失败:{e}")))?;
    if listeners.is_empty() {
        return Ok(Vec::new());
    }
    let names = process_names()
        .map_err(|e| Error::new(ErrorCode::Discover, format!("枚举系统进程失败:{e}")))?;
    let mut out: BTreeMap<u32, ForeignService> = BTreeMap::new();
    for (pid, name) in names {
        let Some(ports) = listeners.get(&pid) else {
            continue;
        };
        out.insert(
            pid,
            ForeignService {
                pid,
                kind: classify_process(&name).to_string(),
                name,
                ports: ports.clone(),
                cwd: None,
                cmd_line: None,
                cpu_percent: None,
                memory_bytes: None,
            },
        );
    }
    // 只对有监听端口的少数 pid 读 PEB 详情；单个 pid 读取失败不影响整体。
    let mut svcs: Vec<ForeignService> = out.into_values().collect();
    for s in &mut svcs {
        if let Some((cwd, cmd_line)) = imp::process_details(s.pid) {
            s.cwd = (!cwd.is_empty()).then_some(cwd);
            s.cmd_line = (!cmd_line.is_empty()).then_some(cmd_line);
        }
        let (cpu_percent, memory_bytes) = imp::process_stats(s.pid);
        s.cpu_percent = cpu_percent;
        s.memory_bytes = memory_bytes;
    }
    imp::prune_cpu_cache(svcs.iter().map(|s| s.pid).collect());
    Ok(svcs)
}

/// 单个 LISTEN 本地端点（地址族信息保留，供健康探测双栈使用）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListenEndpoint {
    pub ip: std::net::IpAddr,
    pub port: u16,
}

/// pid → LISTEN 端点列表（IPv4 + IPv6）。读取失败显式 Err。
pub fn listen_endpoints_by_pid() -> Result<HashMap<u32, Vec<ListenEndpoint>>> {
    imp::listen_endpoints_by_pid()
        .map_err(|e| Error::new(ErrorCode::Discover, format!("读取系统 LISTEN 端口表失败:{e}")))
}

/// port → pid（仅 LISTEN）。
pub fn port_to_pid(port: u16) -> Option<u32> {
    listen_ports_by_pid()
        .ok()?
        .into_iter()
        .find(|(_, ports)| ports.contains(&port))
        .map(|(pid, _)| pid)
}

/// 终止发现列表中的监听进程整棵进程树（发现页「终止」）。
///
/// 护栏：拒绝系统保留 pid（≤4）与 SuperTask 自身；且只允许当前仍持有
/// LISTEN 端口的 pid——把该 IPC 面限制在发现结果内，而非任意进程终止原语。
pub fn kill_tree(pid: u32) -> Result<()> {
    if pid <= 4 {
        return Err(Error::new(
            ErrorCode::JobKill,
            format!("pid {pid} 是系统保留进程，禁止终止"),
        ));
    }
    if pid == std::process::id() {
        return Err(Error::new(ErrorCode::JobKill, "不能终止 SuperTask 自身进程"));
    }
    let listening = listen_ports_by_pid()?
        .get(&pid)
        .is_some_and(|ports| !ports.is_empty());
    if !listening {
        return Err(Error::new(
            ErrorCode::JobKill,
            format!("pid {pid} 不在监听进程列表中，仅允许终止发现到的进程"),
        ));
    }
    taskkill_tree(pid)
}

/// `taskkill /PID <pid> /T /F`（等效杀整棵树）。
/// engine 的外部服务停止与发现页「终止」共用；非 Windows 无 taskkill，no-op。
pub fn taskkill_tree(pid: u32) -> Result<()> {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let status = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(0x0800_0000)
            .status()
            .map_err(|e| Error::new(ErrorCode::JobKill, format!("taskkill 执行失败: {e}")))?;
        if !status.success() {
            return Err(Error::new(
                ErrorCode::JobKill,
                format!("taskkill /PID {pid} 失败（进程可能已退出或权限不足）"),
            ));
        }
    }
    #[cfg(not(windows))]
    {
        let _ = pid;
    }
    Ok(())
}

#[cfg(windows)]
mod imp {
    use std::collections::HashMap;
    use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
    use std::sync::{Mutex, OnceLock};
    use std::time::Instant;

    use netstat2::{
        get_sockets_info, AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo,
    };

    use super::ListenEndpoint;
    use windows::Win32::Foundation::{CloseHandle, FILETIME, HANDLE};
    use windows::Win32::System::Diagnostics::Debug::ReadProcessMemory;
    use windows::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows::Win32::System::ProcessStatus::{GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS};
    use windows::Win32::System::Threading::{
        GetProcessTimes, OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_VM_READ,
    };

    type EndpointMap = HashMap<u32, Vec<ListenEndpoint>>;

    /// CPU 采样缓存：pid → (上次 kernel+user 时间，100ns 单位；上次采样时刻)。
    /// CPU% 需要两次采样求差，首个观察周期返回 None。
    fn cpu_cache() -> &'static Mutex<HashMap<u32, (u64, Instant)>> {
        static CACHE: OnceLock<Mutex<HashMap<u32, (u64, Instant)>>> = OnceLock::new();
        CACHE.get_or_init(|| Mutex::new(HashMap::new()))
    }

    /// 进程消失后清掉缓存条目，避免长期运行时缓慢累积。
    pub fn prune_cpu_cache(keep: Vec<u32>) {
        if let Ok(mut cache) = cpu_cache().lock() {
            cache.retain(|pid, _| keep.contains(pid));
        }
    }

    /// CPU%（整机口径）+ 物理内存（工作集）。进程打不开 / 首次采样 → None。
    pub fn process_stats(pid: u32) -> (Option<f32>, Option<u64>) {
        unsafe {
            let Ok(h) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
                return (None, None);
            };
            if h.is_invalid() {
                return (None, None);
            }
            let cpu = cpu_sample(pid, h);
            let mem = memory_bytes(h);
            let _ = CloseHandle(h);
            (cpu, mem)
        }
    }

    /// 与上次采样求差得到 CPU%；首次见到该 pid 返回 None（只记账）。
    unsafe fn cpu_sample(pid: u32, h: HANDLE) -> Option<f32> {
        let mut creation = FILETIME::default();
        let mut exit = FILETIME::default();
        let mut kernel = FILETIME::default();
        let mut user = FILETIME::default();
        GetProcessTimes(h, &mut creation, &mut exit, &mut kernel, &mut user).ok()?;
        let total = filetime_u64(&kernel).saturating_add(filetime_u64(&user));
        let now = Instant::now();

        let mut cache = cpu_cache().lock().ok()?;
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1);
        match cache.insert(pid, (total, now)) {
            None => None,
            Some((prev_total, prev_at)) => {
                let wall = now.duration_since(prev_at).as_secs_f64();
                if wall <= 0.0 {
                    return None;
                }
                let used_secs = (total.saturating_sub(prev_total) as f64) / 1e7;
                let pct = used_secs / wall / cores as f64 * 100.0;
                Some((pct as f32).clamp(0.0, cores as f32 * 100.0))
            }
        }
    }

    fn filetime_u64(f: &FILETIME) -> u64 {
        ((f.dwHighDateTime as u64) << 32) | f.dwLowDateTime as u64
    }

    /// 物理内存占用（WorkingSetSize，字节）。
    unsafe fn memory_bytes(h: HANDLE) -> Option<u64> {
        let mut pmc = PROCESS_MEMORY_COUNTERS {
            cb: std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            ..Default::default()
        };
        GetProcessMemoryInfo(h, &mut pmc, pmc.cb).ok()?;
        Some(pmc.WorkingSetSize as u64)
    }

    /// pid → LISTEN 本地端点列表（IPv4 + IPv6，去重，v4 优先后按端口升序）。
    pub fn listen_endpoints_by_pid() -> std::io::Result<EndpointMap> {
        let sockets = get_sockets_info(
            AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
            ProtocolFlags::TCP,
        )
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;
        let mut out: EndpointMap = HashMap::new();
        for si in sockets {
            if let ProtocolSocketInfo::Tcp(tcp) = &si.protocol_socket_info {
                if tcp.state != netstat2::TcpState::Listen || tcp.local_port == 0 {
                    continue;
                }
                // 通配符监听（0.0.0.0 / ::，Tomcat 默认双栈）不能作为 connect 目标
                // （10049 地址无效），归一化为回环——通配符监听必然接受回环连接。
                let ip = if tcp.local_addr.is_unspecified() {
                    if tcp.local_addr.is_ipv4() {
                        IpAddr::V4(Ipv4Addr::LOCALHOST)
                    } else {
                        IpAddr::V6(Ipv6Addr::LOCALHOST)
                    }
                } else {
                    tcp.local_addr
                };
                for &pid in &si.associated_pids {
                    if pid != 0 {
                        let v = out.entry(pid).or_default();
                        if !v.contains(&ListenEndpoint { ip, port: tcp.local_port }) {
                            v.push(ListenEndpoint { ip, port: tcp.local_port });
                        }
                    }
                }
            }
        }
        for eps in out.values_mut() {
            // IPv4 优先（health 探测顺序），同族内端口升序
            eps.sort_by_key(|e| (u8::from(e.ip.is_ipv6()), e.port));
        }
        Ok(out)
    }

    pub fn process_names() -> std::io::Result<Vec<(u32, String)>> {
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0).map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::Other, format!("进程快照失败:{e}"))
            })?;
            if snap.is_invalid() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "进程快照句柄无效",
                ));
            }
            let mut out = Vec::new();
            let mut entry = PROCESSENTRY32W {
                dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32,
                ..Default::default()
            };
            if Process32FirstW(snap, &mut entry).is_ok() {
                loop {
                    let len = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..len]);
                    out.push((entry.th32ProcessID, name));
                    if Process32NextW(snap, &mut entry).is_err() {
                        break;
                    }
                }
            }
            let _ = CloseHandle(snap);
            Ok(out)
        }
    }

    #[link(name = "ntdll")]
    extern "system" {
        fn NtQueryInformationProcess(
            handle: HANDLE,
            info_class: i32,
            info: *mut core::ffi::c_void,
            info_len: u32,
            ret_len: *mut u32,
        ) -> i32;
    }

    const PROCESS_BASIC_INFORMATION_CLASS: i32 = 0;

    #[repr(C)]
    struct ProcessBasicInfo {
        exit_status: i32,
        peb_base_address: *mut core::ffi::c_void,
        affinity_mask: usize,
        base_priority: isize,
        unique_process_id: usize,
        inherited_from_unique_process_id: usize,
    }

    /// 读另一进程的 (cwd, cmdline)。同用户进程无需管理员权限；失败返回 None。
    pub fn process_details(pid: u32) -> Option<(String, String)> {
        unsafe {
            let h = OpenProcess(PROCESS_QUERY_INFORMATION | PROCESS_VM_READ, false, pid).ok()?;
            if h.is_invalid() {
                return None;
            }
            let details = read_details(h);
            let _ = CloseHandle(h);
            details
        }
    }

    /// 读 PEB → RTL_USER_PROCESS_PARAMETERS。x64 稳定布局：
    /// ProcessParameters @ PEB+0x20；CurrentDirectory.DosPath @ params+0x38；
    /// CommandLine @ params+0x70。
    unsafe fn read_details(h: HANDLE) -> Option<(String, String)> {
        let mut pbi = ProcessBasicInfo {
            exit_status: 0,
            peb_base_address: std::ptr::null_mut(),
            affinity_mask: 0,
            base_priority: 0,
            unique_process_id: 0,
            inherited_from_unique_process_id: 0,
        };
        let status = NtQueryInformationProcess(
            h,
            PROCESS_BASIC_INFORMATION_CLASS,
            std::ptr::from_mut(&mut pbi).cast(),
            std::mem::size_of_val(&pbi) as u32,
            std::ptr::null_mut(),
        );
        if status != 0 || pbi.peb_base_address.is_null() {
            return None;
        }
        let params = read_u64(h, pbi.peb_base_address as usize + 0x20)? as usize;
        if params == 0 {
            return None;
        }
        let cwd = read_unicode_string(h, params + 0x38).filter(|s| !s.is_empty())?;
        let cmd_line = read_unicode_string(h, params + 0x70);
        Some((cwd, cmd_line.unwrap_or_default()))
    }

    unsafe fn read_u64(h: HANDLE, addr: usize) -> Option<u64> {
        let mut v = 0u64;
        ReadProcessMemory(
            h,
            addr as *const core::ffi::c_void,
            std::ptr::from_mut(&mut v).cast(),
            8,
            None,
        )
        .ok()?;
        Some(v)
    }

    /// 读远进程 UNICODE_STRING { Length: u16, MaximumLength: u16, pad, Buffer }。
    unsafe fn read_unicode_string(h: HANDLE, addr: usize) -> Option<String> {
        let mut header = [0u16; 2];
        ReadProcessMemory(
            h,
            addr as *const core::ffi::c_void,
            header.as_mut_ptr().cast(),
            4,
            None,
        )
        .ok()?;
        let len_bytes = header[0] as usize;
        if len_bytes == 0 || len_bytes % 2 != 0 || len_bytes > 64 * 1024 {
            return None;
        }
        let buf_ptr = read_u64(h, addr + 8)? as usize;
        if buf_ptr == 0 {
            return None;
        }
        let mut chars = vec![0u16; len_bytes / 2];
        ReadProcessMemory(
            h,
            buf_ptr as *const core::ffi::c_void,
            chars.as_mut_ptr().cast(),
            len_bytes,
            None,
        )
        .ok()?;
        Some(String::from_utf16_lossy(&chars))
    }
}

#[cfg(not(windows))]
mod imp {
    use std::collections::HashMap;

    use super::ListenEndpoint;

    type EndpointMap = HashMap<u32, Vec<ListenEndpoint>>;

    pub fn listen_endpoints_by_pid() -> std::io::Result<EndpointMap> {
        Ok(HashMap::new())
    }

    pub fn process_names() -> std::io::Result<Vec<(u32, String)>> {
        Ok(Vec::new())
    }

    pub fn process_details(_pid: u32) -> Option<(String, String)> {
        None
    }

    pub fn process_stats(_pid: u32) -> (Option<f32>, Option<u64>) {
        (None, None)
    }

    pub fn prune_cpu_cache(_keep: Vec<u32>) {}
}

use imp::process_names;

/// pid → LISTEN 本地端口列表（IPv4 + IPv6 合并，升序）。端点实现的折叠视图。
pub fn listen_ports_by_pid() -> Result<HashMap<u32, Vec<u16>>> {
    Ok(
        listen_endpoints_by_pid()?
            .into_iter()
            .map(|(pid, eps)| {
                let mut ports: Vec<u16> = eps.into_iter().map(|e| e.port).collect();
                ports.sort_unstable();
                ports.dedup();
                (pid, ports)
            })
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_matches_prefixes_and_other() {
        assert_eq!(classify_process("javaw.exe"), "java");
        assert_eq!(classify_process("Node.exe"), "node");
        assert_eq!(classify_process("pythonw.exe"), "python");
        assert_eq!(classify_process("bun.exe"), "bun");
        assert_eq!(classify_process("esbuild.exe"), "other");
    }

    #[test]
    fn discover_runs_and_shape_holds() {
        // TCP 表读取失败必须显式报错而非静默空列表。
        let svcs = discover_services().expect("本机 LISTEN 端口表读取不应失败");
        for s in &svcs {
            assert!(s.pid > 0);
            assert!(!s.name.is_empty());
            assert!(!s.ports.is_empty());
            assert!(!s.kind.is_empty());
            if let Some(pct) = s.cpu_percent {
                assert!((0.0..=1600.0).contains(&pct), "cpu% 越界: {pct}");
            }
        }
    }

    #[test]
    fn kill_tree_guardrails() {
        // 系统保留 pid / SuperTask 自身 / 非监听 pid 一律拒绝
        assert!(kill_tree(0).is_err());
        assert!(kill_tree(4).is_err(), "System pid 必须拒绝");
        assert!(kill_tree(std::process::id()).is_err(), "自身进程必须拒绝");
        assert!(
            kill_tree(u32::MAX - 1).is_err(),
            "非监听 pid 必须拒绝（仅允许终止发现到的进程）"
        );
    }

    #[cfg(windows)]
    #[test]
    fn taskkill_tree_kills_child_process() {
        let mut child = std::process::Command::new("ping")
            .args(["-n", "30", "127.0.0.1"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .expect("spawn ping");
        let pid = child.id();
        taskkill_tree(pid).expect("taskkill 应成功");
        let status = child.wait().unwrap();
        assert!(!status.success(), "子进程应被强制终止");
    }

    #[test]
    fn cpu_second_sample_yields_percent() {
        // CPU% 是差分采样：第二次 discover 至少应有部分进程给出读数
        // （System 等受保护进程会拒绝读取，按设计返回 None）。
        let _ = discover_services();
        std::thread::sleep(std::time::Duration::from_millis(60));
        let svcs = discover_services().expect("本机 LISTEN 端口表读取不应失败");
        let with_cpu = svcs.iter().filter(|s| s.cpu_percent.is_some()).count();
        let with_mem = svcs.iter().filter(|s| s.memory_bytes.is_some()).count();
        assert!(with_cpu >= 1, "第二次采样没有任何进程拿到 CPU%（共 {} 个）", svcs.len());
        assert!(with_mem >= 1, "没有任何进程拿到内存读数（共 {} 个）", svcs.len());
    }

    #[test]
    fn ipv4_listener_also_matches() {
        // 回归：本地 IPv4 监听端口要能反查到 pid（曾因表解析错误全部失配）
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_to_pid(port).is_some(), "IPv4 监听端口 {port} 必须能反查到 pid");
    }

    #[test]
    fn ipv6_listener_also_matches() {
        // 回归：Node 默认监听 [::]，IPv6 监听必须能被发现
        let listener = std::net::TcpListener::bind("[::1]:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        assert!(port_to_pid(port).is_some(), "IPv6 监听端口 {port} 必须能反查到 pid");
    }

    #[test]
    fn wildcard_listener_normalized_to_loopback() {
        // 回归：Tomcat 默认监听 [::]（双栈任意地址），TCP 表返回通配符地址；
        // 直接 connect 通配符报 10049（地址无效），必须归一化为回环，
        // 否则服务已在运行却永远探测失败 → 运行页一直「不健康」。
        let listener = std::net::TcpListener::bind("[::]:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || loop {
            let _ = listener.accept();
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        let m = listen_endpoints_by_pid().expect("端点表读取不应失败");
        let mine = m.get(&std::process::id()).cloned().unwrap_or_default();
        let ep = mine
            .iter()
            .find(|e| e.port == port)
            .unwrap_or_else(|| panic!("通配符监听 {port} 应被发现: {mine:?}"));
        assert!(
            !ep.ip.is_unspecified(),
            "端点必须是回环而非通配符: {:?}",
            ep.ip
        );
        // 端点感知探测应能连通（运行页健康检查的真实路径）
        let spec = crate::spec::HealthSpec {
            r#type: crate::spec::HealthType::Tcp,
            http: None,
            interval_secs: 2,
            timeout_secs: 2,
        };
        let r = crate::health::check_with_endpoints(&spec, Some(port), &mine);
        assert!(r.ok, "归一化后探测应连通: {}", r.detail);
    }

    #[test]
    fn endpoints_keep_address_family() {
        // 回归：健康探测依赖端点的地址族信息，[::1] 监听必须带 ip=::1
        let listener = std::net::TcpListener::bind("[::1]:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _ = listener.accept();
        });
        std::thread::sleep(std::time::Duration::from_millis(30));
        let m = listen_endpoints_by_pid().expect("端点表读取不应失败");
        let mine = m.get(&std::process::id()).cloned().unwrap_or_default();
        assert!(
            mine.iter().any(|e| e.port == port && e.ip.is_ipv6()),
            "本进程 [::1]:{port} 端点未被发现: {mine:?}"
        );
    }
}
