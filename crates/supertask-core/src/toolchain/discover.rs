//! 本机已装工具链枚举（P1，2026-09-01）。只读：不写任何配置、不改 PATH、
//! 绝不调用 `nvm.exe`（输出本地化、交互脆弱；文件扫描是确定性的），
//! 绝不改写 NVM_SYMLINK / 全局状态。
//!
//! 候选源对齐 IDEA / VS Code java 插件的自研清单：
//! - Java：Windows 注册表（JavaSoft JDK 9+ / Java Development Kit 8 / Adoptium /
//!   Microsoft JDK，含 WOW6432Node）+ `JAVA_HOME` + 常见安装目录
//!   （Program Files\Java、Eclipse Adoptium、Corretto、Zulu、`~/.jdks`；
//!   Unix：`/usr/lib/jvm`、sdkman、macOS JavaVirtualMachines）。
//! - Node：nvm-windows `NVM_HOME`（settings.txt root 优先）扫 `v*` 目录，
//!   `NVM_SYMLINK` 实链目标即 active；Unix 兼容 `~/.nvm/versions/node`。
//! - Maven：`MAVEN_HOME` / `M2_HOME`、PATH 命中目录与常见安装目录。
//!
//! 每个 Java 候选 home 经 `bin/java -version` spawn 验证（复用
//! [`crate::probe::version_of`]，4s 超时），半截安装直接丢弃；nvm 目录名
//! 即版本号（nvm-windows 自身创建），只查 node 二进制存在性，不再 spawn。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::ToolKind;

/// 单个已装安装。`active` = PATH / nvm symlink 当前指向的那份（与
/// `toolchain.probe` 的工具项对齐，见 [`crate::probe::probe_bundle`] 的补充判定）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscoveredInstall {
    pub tool: ToolKind,
    /// 裸版本号：Java 来自 `-version` 输出解析；nvm 来自目录名。
    pub version: String,
    /// 安装根目录（JDK home / nvm `v*` 目录），非可执行文件路径。
    pub home: String,
    pub source: InstallSource,
    /// 当前 PATH / symlink 指向的安装。
    pub active: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InstallSource {
    /// Windows 注册表注册的 JDK。
    Registry,
    /// 常见安装目录扫描命中。
    Directory,
    /// `JAVA_HOME` 环境变量指向。
    EnvVar,
    /// nvm 版本管理器目录（nvm-windows / `~/.nvm`）。
    NvmDir,
}

/// 全量发现入口：java + node + maven 并行（照抄 `probe_toolchain` 的 thread::scope
/// 模式）。总耗时 ≈ 最慢单个验证（每个 home 一次 4s 封顶的 spawn）。
pub fn discover_installed() -> Vec<DiscoveredInstall> {
    let (java, node, maven) = std::thread::scope(|s| {
        let j = s.spawn(discover_java);
        let n = s.spawn(discover_node);
        let m = s.spawn(discover_maven);
        (
            j.join().unwrap_or_default(),
            n.join().unwrap_or_default(),
            m.join().unwrap_or_default(),
        )
    });
    let mut out = java;
    out.extend(node);
    out.extend(maven);
    out
}

// ---------------------------------------------------------------------------
// Java
// ---------------------------------------------------------------------------

/// 候选源顺序：注册表（Windows）→ JAVA_HOME → 目录扫描。canonical home 去重；
/// 版本一律以 `bin/java -version` 输出为准（目录名不做假设：`jdk-1.8` /
/// `jdk-25` / `temurin-21-container` 都收）。
pub fn discover_java() -> Vec<DiscoveredInstall> {
    let mut homes: Vec<(PathBuf, InstallSource)> = Vec::new();
    let mut push = |home: PathBuf, source: InstallSource| {
        if home.is_dir() && !homes.iter().any(|(h, _)| h == &home) {
            homes.push((home, source));
        }
    };

    for (home, source) in java_registry_homes() {
        push(home, source);
    }
    if let Some(home) = java_home_env() {
        push(home, InstallSource::EnvVar);
    }
    for (home, source) in java_dir_candidates() {
        push(home, source);
    }

    // 逐个验证：`<home>/bin/java -version` 提取版本；失败（半截安装/损坏）丢弃。
    let verified: Vec<Option<(String, PathBuf, InstallSource)>> = std::thread::scope(|s| {
        homes
            .iter()
            .map(|(home, source)| {
                s.spawn(move || {
                    verify_java_home(home).map(|version| (version, home.clone(), *source))
                })
            })
            .collect::<Vec<_>>()
            .into_iter()
            .map(|h| h.join().unwrap_or(None))
            .collect()
    });

    let mut installs: Vec<DiscoveredInstall> = verified
        .into_iter()
        .flatten()
        .map(|(version, home, source)| DiscoveredInstall {
            tool: ToolKind::Java,
            version,
            home: home.display().to_string(),
            source,
            active: false,
        })
        .collect();
    dedup_java_same_version(&mut installs);
    sort_by_version_desc(&mut installs);
    installs
}

/// 同版本去重（JDK 目录与 JRE 常报同一版本号，如 `jdk-1.8` 与 `jre-1.8` →
/// `1.8.0_371`）：保留首个含 javac 的（JDK 优于 JRE）；都无 javac 保首个。
/// 去重后仍按版本降序的相对顺序稳定。
fn dedup_java_same_version(installs: &mut Vec<DiscoveredInstall>) {
    installs.sort_by_key(|i| !has_javac(Path::new(&i.home))); // 稳定排序：JDK 靠前
    let mut seen: Vec<String> = Vec::new();
    installs.retain(|i| {
        if seen
            .iter()
            .any(|v: &String| v.eq_ignore_ascii_case(&i.version))
        {
            false
        } else {
            seen.push(i.version.clone());
            true
        }
    });
}

/// home/bin 下 javac 存在性（JDK 与纯 JRE 的判别）。
fn has_javac(home: &Path) -> bool {
    ["javac.exe", "javac"]
        .iter()
        .any(|n| home.join("bin").join(n).is_file())
}

/// 注册表候选源。键名固定、值来自 OS，非用户输入；读取失败静默跳过。
#[cfg(windows)]
fn java_registry_homes() -> Vec<(PathBuf, InstallSource)> {
    const ROOTS: &[&str] = &[
        // 9+：`HKLM\SOFTWARE\JavaSoft\JDK\<ver>`，值 JavaHome
        r"SOFTWARE\JavaSoft\JDK",
        // JDK 8 专用键名（64 位视图）
        r"SOFTWARE\JavaSoft\Java Development Kit",
        // 32 位 JDK 8 在 64 位系统上的视图
        r"SOFTWARE\WOW6432Node\JavaSoft\Java Development Kit",
        // Temurin / Microsoft Build of OpenJDK
        r"SOFTWARE\Eclipse Adoptium\JDK",
        r"SOFTWARE\Microsoft\JDK",
    ];
    let mut out = Vec::new();
    for root in ROOTS {
        let Some(subkeys) = reg_enum_subkeys(root) else {
            continue;
        };
        for sub in subkeys {
            let Some(home) = super::resolver::read_reg_value(&format!(r"{root}\{sub}"), "JavaHome")
            else {
                continue;
            };
            out.push((PathBuf::from(home), InstallSource::Registry));
        }
    }
    out
}

#[cfg(not(windows))]
fn java_registry_homes() -> Vec<(PathBuf, InstallSource)> {
    Vec::new()
}

fn java_home_env() -> Option<PathBuf> {
    let v = std::env::var("JAVA_HOME").ok()?;
    let v = v.trim();
    if v.is_empty() {
        return None;
    }
    Some(PathBuf::from(v))
}

/// 常见安装目录扫描。只扫一层子目录，目录名不假设版本语义。
fn java_dir_candidates() -> Vec<(PathBuf, InstallSource)> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if cfg!(windows) {
        if let Some(pf) = std::env::var_os("ProgramFiles") {
            let pf = PathBuf::from(pf);
            for name in [
                "Java",
                "Eclipse Adoptium",
                "Amazon Corretto",
                "Zulu",
                "BellSoft",
            ] {
                roots.push(pf.join(name));
            }
        }
        if let Some(home) = std::env::var_os("USERPROFILE") {
            // IDEA 自己的下载目录
            roots.push(PathBuf::from(home).join(".jdks"));
        }
    } else {
        #[cfg(target_os = "linux")]
        roots.push(PathBuf::from("/usr/lib/jvm"));
        #[cfg(target_os = "macos")]
        roots.push(PathBuf::from("/Library/Java/JavaVirtualMachines"));
        if let Some(home) = std::env::var_os("HOME") {
            let home = PathBuf::from(home);
            roots.push(home.join(".sdkman/candidates/java"));
            #[cfg(target_os = "macos")]
            roots.push(home.join("Library/Java/JavaVirtualMachines"));
        }
    }

    let mut out = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            // macOS 布局：`<name>/Contents/Home` 才是 JDK home
            let mac_home = entry.path().join("Contents/Home");
            if mac_home.is_dir() {
                out.push((mac_home, InstallSource::Directory));
            } else if entry.path().is_dir() {
                out.push((entry.path(), InstallSource::Directory));
            }
        }
    }
    out
}

/// 验证候选 java home：`bin/java -version` 提取版本；无二进制或验证失败 → None。
fn verify_java_home(home: &Path) -> Option<String> {
    let exe = home
        .join("bin")
        .join(if cfg!(windows) { "java.exe" } else { "java" });
    if !exe.is_file() {
        return None;
    }
    crate::probe::version_of(&exe, &["-version"])
}

// ---------------------------------------------------------------------------
// Node / nvm
// ---------------------------------------------------------------------------

/// nvm 布局发现。Windows：`NVM_HOME`（settings.txt root 优先）下的 `v*` 目录，
/// `NVM_SYMLINK` 实链目标即 active。Unix：`~/.nvm/versions/node`（active 由
/// probe_bundle 的版本比对兜底）。均无 → PATH 上的 node 视为唯一安装兜底
/// （系统安装 / volta / fnm shim）。
pub fn discover_node() -> Vec<DiscoveredInstall> {
    let mut installs = scan_nvm_installs();

    // 非 nvm 兜底：PATH 上（或 standalone 目录）的 node。有 nvm 安装时也补
    // PATH 命中但不在 nvm 目录里的情况（如独立安装的 node.exe 在前）。
    let path_node = crate::probe::find_on_path(if cfg!(windows) { "node.exe" } else { "node" });
    if let Some(exe) = path_node {
        if let Some(home) = exe.parent().map(Path::to_path_buf) {
            let in_nvm = installs.iter().any(|i| same_dir(Path::new(&i.home), &home));
            if !in_nvm {
                if let Some(version) = verify_node_home(&home) {
                    installs.push(DiscoveredInstall {
                        tool: ToolKind::Node,
                        version,
                        home: home.display().to_string(),
                        source: InstallSource::Directory,
                        active: true,
                    });
                }
            }
        }
    }

    let mut out = installs;
    sort_by_version_desc(&mut out);
    out
}

/// 扫 nvm 目录布局。目录名 `v14.21.3` → 版本 `14.21.3`；无 node 二进制的目录跳过。
fn scan_nvm_installs() -> Vec<DiscoveredInstall> {
    let mut out = Vec::new();
    for root in nvm_roots() {
        for (home, version) in scan_nvm_dirs(&root) {
            let active = nvm_symlink_target().is_some_and(|t| same_dir(&home, &t));
            out.push(DiscoveredInstall {
                tool: ToolKind::Node,
                version,
                home: home.display().to_string(),
                source: InstallSource::NvmDir,
                active,
            });
        }
    }
    out
}

/// nvm 根目录候选：Windows 的 NVM_HOME（settings.txt root 优先），Unix 的
/// `~/.nvm/versions/node`。目录不存在 → 跳过。
fn nvm_roots() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if cfg!(windows) {
        if let Some(home) = std::env::var("NVM_HOME")
            .ok()
            .map(|v| PathBuf::from(v.trim()))
        {
            if let Some(root) = nvm_root_from_settings(&home) {
                out.push(root);
            } else {
                out.push(home);
            }
        }
    }
    if let Some(home) = std::env::var_os("HOME") {
        out.push(PathBuf::from(home).join(".nvm/versions/node"));
    }
    out.into_iter().filter(|p| p.is_dir()).collect()
}

/// 目录名 → nvm 版本号（`v14.21.3` → `14.21.3`）。首段必须是数字，排除
/// `vapp` 之类非版本目录。
fn nvm_version_from_dir_name(name: &str) -> Option<String> {
    let ver = name.strip_prefix('v')?;
    if ver.is_empty() || !ver.chars().next().is_some_and(|c| c.is_ascii_digit()) {
        return None;
    }
    // 其余字符限定版本字符集（数字/点/连字符），防奇异目录名混入显示
    if !ver
        .chars()
        .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
    {
        return None;
    }
    Some(ver.to_string())
}

/// 扫 nvm root 下的版本目录，按版本升序返回（home, version）。
fn scan_nvm_dirs(root: &Path) -> Vec<(PathBuf, String)> {
    let Ok(rd) = std::fs::read_dir(root) else {
        return Vec::new();
    };
    let mut dirs: Vec<(PathBuf, String)> = rd
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_str()?.to_string();
            let ver = nvm_version_from_dir_name(&name)?;
            let home = e.path();
            node_exe_in(&home)?;
            Some((home, ver))
        })
        .collect();
    dirs.sort_by(|a, b| nvm_ver_cmp(&a.1, &b.1));
    dirs
}

/// settings.txt 内容 → nvm root。只认 `root: <path>` 行；无/解析失败 → None。
fn nvm_root_from_settings(nvm_home: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(nvm_home.join("settings.txt")).ok()?;
    for line in text.lines() {
        if let Some(rest) = line.trim().strip_prefix("root:") {
            let p = rest.trim();
            if !p.is_empty() {
                return Some(PathBuf::from(p));
            }
        }
    }
    None
}

/// NVM_SYMLINK（如 `C:\Program Files\nodejs`）→ 实链目标。canonicalize 穿透
/// symlink/junction；失败时退回原始路径（active 判定退化为字符串比较）。
fn nvm_symlink_target() -> Option<PathBuf> {
    let link = std::env::var("NVM_SYMLINK").ok()?;
    let link = PathBuf::from(link.trim());
    if !link.exists() {
        return None;
    }
    Some(std::fs::canonicalize(&link).unwrap_or(link))
}

/// home（或其 bin/）下 node 可执行文件存在性。两种名字在所有平台都查
/// （Windows 布局 `node.exe`、Unix 布局 `bin/node`，交叉兼容无害）。
fn node_exe_in(home: &Path) -> Option<PathBuf> {
    for name in ["node.exe", "node"] {
        let direct = home.join(name);
        if direct.is_file() {
            return Some(direct);
        }
        let in_bin = home.join("bin").join(name);
        if in_bin.is_file() {
            return Some(in_bin);
        }
    }
    None
}

/// 验证候选 node home：`node -v` 提取版本（PATH 兜底分支用；nvm 目录名可信
/// 不再 spawn）。
fn verify_node_home(home: &Path) -> Option<String> {
    let exe = node_exe_in(home)?;
    crate::probe::version_of(&exe, &["-v"])
}

// ---------------------------------------------------------------------------
// Maven
// ---------------------------------------------------------------------------

/// Maven installations do not have a standard registry schema. Discover the
/// explicit home variables, the active PATH installation, and conventional
/// Apache Maven directories, then verify each home by running `mvn -v`.
pub fn discover_maven() -> Vec<DiscoveredInstall> {
    let mut homes: Vec<(PathBuf, InstallSource)> = Vec::new();
    let mut push = |home: PathBuf, source: InstallSource| {
        if home.is_dir() && !homes.iter().any(|(h, _)| h == &home) {
            homes.push((home, source));
        }
    };

    for key in ["MAVEN_HOME", "M2_HOME"] {
        if let Ok(value) = std::env::var(key) {
            let value = value.trim();
            if !value.is_empty() {
                push(PathBuf::from(value), InstallSource::EnvVar);
            }
        }
    }
    if let Some(exe) =
        crate::probe::find_on_path("mvn.cmd").or_else(|| crate::probe::find_on_path("mvn"))
    {
        if let Some(bin) = exe.parent() {
            if let Some(home) = bin.parent() {
                push(home.to_path_buf(), InstallSource::Directory);
            }
        }
    }
    for root in maven_dir_roots() {
        if root
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("maven"))
        {
            push(root.clone(), InstallSource::Directory);
        }
        let Ok(rd) = std::fs::read_dir(&root) else {
            continue;
        };
        for entry in rd.flatten() {
            if entry.path().is_dir() {
                push(entry.path(), InstallSource::Directory);
            }
        }
    }

    let mut installs: Vec<DiscoveredInstall> = homes
        .into_iter()
        .filter_map(|(home, source)| {
            verify_maven_home(&home).map(|version| DiscoveredInstall {
                tool: ToolKind::Maven,
                version,
                home: home.display().to_string(),
                source,
                active: false,
            })
        })
        .collect();
    installs.sort_by(|a, b| nvm_ver_cmp(&b.version, &a.version));
    installs
}

fn maven_dir_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if cfg!(windows) {
        for key in ["ProgramFiles", "ProgramFiles(x86)"] {
            if let Some(value) = std::env::var_os(key) {
                let root = PathBuf::from(value);
                roots.push(root.join("Maven"));
                roots.push(root.join("Apache").join("Maven"));
                roots.push(root);
            }
        }
    } else {
        roots.extend([
            PathBuf::from("/usr/share/maven"),
            PathBuf::from("/opt"),
            PathBuf::from("/usr/local"),
        ]);
        if let Some(home) = std::env::var_os("HOME") {
            roots.push(PathBuf::from(home).join(".sdkman/candidates/maven"));
        }
    }
    roots
}

fn verify_maven_home(home: &Path) -> Option<String> {
    let names = if cfg!(windows) {
        ["bin/mvn.cmd", "bin/mvn.bat", "bin/mvn.exe"]
    } else {
        ["bin/mvn", "bin/mvn.cmd", "bin/mvn.bat"]
    };
    names
        .iter()
        .map(|name| home.join(name))
        .find(|path| path.is_file())
        .and_then(|path| crate::probe::version_of(&path, &["-v"]))
}

// ---------------------------------------------------------------------------
// 共用小件
// ---------------------------------------------------------------------------

/// 版本号比较：按 `.` 分段逐段数值比，非数字段退化字典序
/// （`14.21.3` < `24.9.0`；`1.8.0_392` 的 `0_392` 段走字符串比）。
fn nvm_ver_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    let mut sa = a.trim_start_matches('v').split('.');
    let mut sb = b.trim_start_matches('v').split('.');
    loop {
        match (sa.next(), sb.next()) {
            (None, None) => return std::cmp::Ordering::Equal,
            (None, Some(_)) => return std::cmp::Ordering::Less,
            (Some(_), None) => return std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => {
                let ord = match (x.parse::<u64>(), y.parse::<u64>()) {
                    (Ok(x), Ok(y)) => x.cmp(&y),
                    _ => x.cmp(y),
                };
                if ord != std::cmp::Ordering::Equal {
                    return ord;
                }
            }
        }
    }
}

fn sort_by_version_desc(installs: &mut [DiscoveredInstall]) {
    installs.sort_by(|a, b| nvm_ver_cmp(&b.version, &a.version));
}

/// 目录等价判断：canonicalize 穿透 symlink/junction 后比较；失败退回
/// 大小写不敏感字符串比较（Windows 路径大小写不敏感）。
fn same_dir(a: &Path, b: &Path) -> bool {
    if let (Ok(a), Ok(b)) = (std::fs::canonicalize(a), std::fs::canonicalize(b)) {
        return a == b;
    }
    a.to_string_lossy()
        .eq_ignore_ascii_case(&b.to_string_lossy())
}

/// `reg query <key>`（无 /v）枚举子键。输出形如：
/// ```text
/// HKEY_LOCAL_MACHINE\SOFTWARE\JavaSoft\JDK
///
/// HKEY_LOCAL_MACHINE\SOFTWARE\JavaSoft\JDK\11.0.20
/// HKEY_LOCAL_MACHINE\SOFTWARE\JavaSoft\JDK\17.0.7
/// ```
/// 键不存在 / reg 不可用 → None。输出格式非本地化（HKEY_ 前缀固定）。
#[cfg(windows)]
fn reg_enum_subkeys(key: &str) -> Option<Vec<String>> {
    let out = std::process::Command::new("reg")
        .args(["query", key])
        .creation_flags_no_window()
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    Some(parse_reg_subkeys(key, &text))
}

/// 解析 `reg query` 输出中被查询键之下的子键名（纯函数，测试注入文本）。
#[cfg(windows)]
fn parse_reg_subkeys(key: &str, output: &str) -> Vec<String> {
    let key = key.trim_end_matches('\\');
    let prefix_full = format!(r"HKEY_LOCAL_MACHINE\{key}");
    let mut out = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix(&prefix_full) else {
            continue;
        };
        // 直接子键：`<prefix>\<name>`，name 不含分隔符
        let Some(name) = rest.strip_prefix('\\') else {
            continue; // 被查询键自身的行
        };
        if !name.is_empty() && !name.contains('\\') && !out.iter().any(|x: &String| x == name) {
            out.push(name.to_string());
        }
    }
    out
}

#[cfg(windows)]
trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

#[cfg(windows)]
impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        {
            use std::os::windows::process::CommandExt;
            self.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nvm_dir_name_parses_versions_only() {
        assert_eq!(
            nvm_version_from_dir_name("v14.21.3").as_deref(),
            Some("14.21.3")
        );
        assert_eq!(nvm_version_from_dir_name("v24").as_deref(), Some("24"));
        assert_eq!(
            nvm_version_from_dir_name("v24.19.0").as_deref(),
            Some("24.19.0")
        );
    }

    #[test]
    fn nvm_dir_name_rejects_non_versions() {
        assert_eq!(nvm_version_from_dir_name("vapp"), None);
        assert_eq!(nvm_version_from_dir_name("node.exe"), None);
        assert_eq!(nvm_version_from_dir_name("v"), None);
        assert_eq!(nvm_version_from_dir_name(""), None);
        assert_eq!(nvm_version_from_dir_name("v24x"), None);
    }

    #[test]
    fn version_cmp_numeric_per_segment() {
        use std::cmp::Ordering::*;
        assert_eq!(nvm_ver_cmp("14.21.3", "24.9.0"), Less);
        assert_eq!(nvm_ver_cmp("24.9.0", "24.19.0"), Less); // 数值比，非字典序
        assert_eq!(nvm_ver_cmp("24.19.0", "24.19.0"), Equal);
        assert_eq!(nvm_ver_cmp("25", "24.9.0"), Greater);
        // 非数字段退化字典序
        assert_eq!(nvm_ver_cmp("1.8.0_392", "1.8.0_371"), Greater);
    }

    #[test]
    fn sort_desc_puts_newest_first() {
        let mut v = vec![
            DiscoveredInstall {
                tool: ToolKind::Node,
                version: "14.21.3".into(),
                home: "a".into(),
                source: InstallSource::NvmDir,
                active: false,
            },
            DiscoveredInstall {
                tool: ToolKind::Node,
                version: "24.19.0".into(),
                home: "b".into(),
                source: InstallSource::NvmDir,
                active: false,
            },
        ];
        sort_by_version_desc(&mut v);
        assert_eq!(v[0].version, "24.19.0");
        assert_eq!(v[1].version, "14.21.3");
    }

    #[test]
    fn nvm_root_settings_line_wins() {
        let dir = std::env::temp_dir().join(format!("st-discover-{}", std::process::id()));
        let home = dir.join("nvm-home");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("settings.txt"),
            "root: C:\\custom\\nvm-root\narch: 64\nproxy: none\n",
        )
        .unwrap();
        let root = nvm_root_from_settings(&home).unwrap();
        assert_eq!(root, PathBuf::from("C:\\custom\\nvm-root"));

        // 无 settings.txt → None（调用方回退 NVM_HOME 本身）
        let empty = dir.join("nvm-empty");
        std::fs::create_dir_all(&empty).unwrap();
        assert_eq!(nvm_root_from_settings(&empty), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn scan_nvm_dirs_finds_version_dirs_with_node_binary_only() {
        let dir = std::env::temp_dir().join(format!("st-discover-scan-{}", std::process::id()));
        let root = dir.join("nvm");
        for v in ["v14.21.3", "v24.19.0", "v9.11.1"] {
            std::fs::create_dir_all(root.join(v)).unwrap();
        }
        // 带二进制的版本（nvm-windows 布局：根下直接 node.exe）
        std::fs::write(root.join("v14.21.3").join("node.exe"), b"fake").unwrap();
        std::fs::write(root.join("v24.19.0").join("node.exe"), b"fake").unwrap();
        // v9.11.1 无二进制 → 跳过；非版本目录 → 跳过
        std::fs::create_dir_all(root.join("vapp")).unwrap();

        let found = scan_nvm_dirs(&root);
        let vers: Vec<&str> = found.iter().map(|(_, v)| v.as_str()).collect();
        assert_eq!(vers, vec!["14.21.3", "24.19.0"]); // 升序；无二进制的被滤掉
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unix_nvm_layout_bin_node_also_found() {
        let dir = std::env::temp_dir().join(format!("st-discover-unix-{}", std::process::id()));
        let root = dir.join("node");
        std::fs::create_dir_all(root.join("v20.18.1/bin")).unwrap();
        std::fs::write(root.join("v20.18.1/bin/node"), b"fake").unwrap();
        let found = scan_nvm_dirs(&root);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1, "20.18.1");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn reg_subkeys_parsed_from_query_output() {
        let out = "HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\n\n\
                   HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\\11.0.20\n\
                   HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\\17.0.7\n\
                   HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\\21.0.3\n";
        let keys = parse_reg_subkeys(r"SOFTWARE\JavaSoft\JDK", out);
        assert_eq!(keys, vec!["11.0.20", "17.0.7", "21.0.3"]);
        // 被查询键自身行、孙键行不收
        let with_grandchild = "HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\n\
             HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\\25.0.1\n\
             HKEY_LOCAL_MACHINE\\SOFTWARE\\JavaSoft\\JDK\\25.0.1\\MSI\n";
        assert_eq!(
            parse_reg_subkeys(r"SOFTWARE\JavaSoft\JDK", with_grandchild),
            vec!["25.0.1"]
        );
        assert!(parse_reg_subkeys(r"SOFTWARE\NoSuchKey", out).is_empty());
    }

    /// 真机冒烟：结构合法即可，不假设具体装了什么（CI 上可能什么都没有）。
    #[test]
    fn discover_runs_and_shape_is_valid() {
        let all = discover_installed();
        for i in &all {
            assert!(!i.version.is_empty());
            assert!(!i.home.is_empty());
        }
        // 版本降序且无重复版本（同工具内）
        for pair in all.windows(2) {
            if pair[0].tool == pair[1].tool {
                assert_eq!(
                    nvm_ver_cmp(&pair[0].version, &pair[1].version),
                    std::cmp::Ordering::Greater,
                    "{:?} 应排在 {:?} 前",
                    pair[0].version,
                    pair[1].version
                );
            }
        }
    }
}
