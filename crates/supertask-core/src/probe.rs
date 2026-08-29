use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProbe {
    pub found: bool,
    pub version: Option<String>,
    pub path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolchainProbe {
    pub java: ToolProbe,
    pub maven: ToolProbe,
    /// 1.4 §5.4：仅信息展示（wrapper 是唯一推荐执行方式），不提供安装入口。
    pub gradle: ToolProbe,
    pub node: ToolProbe,
    pub npm: ToolProbe,
    pub pnpm: ToolProbe,
    pub yarn: ToolProbe,
    /// 1.6 §6.2：网关三引擎探测（不代装，缺失给平台指引）。
    #[serde(default)]
    pub gateway: crate::gateway::probe::GatewayProbe,
}

/// Hard ceiling for a single tool probe. A healthy tool answers in <1s; a
/// hung/corrupt one (e.g. a broken `mvn` on PATH) is killed and reported as
/// not found instead of blocking `app.load` for minutes.
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

pub fn probe_toolchain() -> ToolchainProbe {
    // Probe every tool on its own thread so the total time is bounded by the
    // slowest single tool, not the sum of them.
    let (java, maven, gradle, node, npm, pnpm, yarn, gw) = std::thread::scope(|s| {
        let java = s.spawn(|| probe_one(&["java.exe", "java"], &["-version"]));
        let maven = s.spawn(|| probe_one(&["mvn.cmd", "mvn.bat", "mvn.exe", "mvn"], &["-v"]));
        let gradle = s.spawn(|| {
            probe_one(&["gradle.bat", "gradle.exe", "gradle"], &["--version"])
        });
        let node = s.spawn(|| probe_one(&["node.exe", "node"], &["-v"]));
        let npm = s.spawn(|| probe_one(&["npm.cmd", "npm.exe", "npm"], &["-v"]));
        let pnpm = s.spawn(|| probe_one(&["pnpm.cmd", "pnpm.exe", "pnpm"], &["-v"]));
        let yarn = s.spawn(|| probe_one(&["yarn.cmd", "yarn.exe", "yarn"], &["-v"]));
        let gw = s.spawn(crate::gateway::probe::probe_gateway);
        (
            java.join().unwrap_or_default(),
            maven.join().unwrap_or_default(),
            gradle.join().unwrap_or_default(),
            node.join().unwrap_or_default(),
            npm.join().unwrap_or_default(),
            pnpm.join().unwrap_or_default(),
            yarn.join().unwrap_or_default(),
            gw.join().unwrap_or_default(),
        )
    });
    ToolchainProbe {
        java,
        maven,
        gradle,
        node,
        npm,
        pnpm,
        yarn,
        gateway: gw,
    }
}

/// Resolve a planned program name against PATH (Windows PATHEXT-aware).
pub fn resolve_program(name: &str) -> Result<PathBuf> {
    if let Some(p) = find_on_path(name) {
        return Ok(p);
    }
    // 1.4 §4.3：PATH 未命中时补平台已知位置候选（只读，不写 shell 配置、不改 PATH）
    for dir in platform_known_dirs() {
        let p = dir.join(name);
        if p.is_file() {
            return Ok(p);
        }
    }
    Err(Error::new(
        ErrorCode::MissingTool,
        format!("未找到 {name}。请安装并确保在 PATH 中，或在「环境」页一键安装。"),
    ))
}

/// 平台已知位置（仅 Unix；Windows 只认 PATH，行为零变化）。
#[cfg(not(windows))]
fn platform_known_dirs() -> Vec<PathBuf> {
    let mut out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/opt/homebrew/bin"));
        out.push(PathBuf::from("/usr/local/bin"));
        // sdkman：~/.sdkman/candidates/<tool>/<version>/bin（任一已装版本）
        if let Some(home) = std::env::var_os("HOME") {
            let cands = PathBuf::from(&home).join(".sdkman/candidates");
            if let Ok(rd) = std::fs::read_dir(&cands) {
                for tool in rd.flatten() {
                    if let Ok(vs) = std::fs::read_dir(tool.path()) {
                        for v in vs.flatten() {
                            out.push(v.path().join("bin"));
                        }
                    }
                }
            }
            // nvm：~/.nvm/versions/node/<version>/bin
            let nvm = PathBuf::from(&home).join(".nvm/versions/node");
            if let Ok(vs) = std::fs::read_dir(&nvm) {
                for v in vs.flatten() {
                    out.push(v.path().join("bin"));
                }
            }
        }
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(home) = std::env::var_os("HOME") {
            out.push(PathBuf::from(&home).join(".local/share/mise/shims"));
        }
        // 系统包管理器装的 JVM：/usr/lib/jvm/<dir>/bin
        if let Ok(rd) = std::fs::read_dir("/usr/lib/jvm") {
            for v in rd.flatten() {
                out.push(v.path().join("bin"));
            }
        }
    }
    out
}

#[cfg(windows)]
fn platform_known_dirs() -> Vec<PathBuf> {
    Vec::new()
}

pub fn find_on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    let mut names = vec![name.to_string()];
    if !name.contains('.') {
        for ext in pathext() {
            names.push(format!("{name}{ext}"));
        }
    }
    for dir in std::env::split_paths(&path) {
        for n in &names {
            let p = dir.join(n);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

fn pathext() -> Vec<String> {
    std::env::var("PATHEXT")
        .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".into())
        .split(';')
        .filter(|s| !s.is_empty())
        .map(|s| {
            if s.starts_with('.') {
                s.to_string()
            } else {
                format!(".{s}")
            }
        })
        .collect()
}

fn probe_one(candidates: &[&str], args: &[&str]) -> ToolProbe {
    for name in candidates {
        if let Some(path) = find_on_path(name) {
            return match version_of(&path, args) {
                // Present and responsive → found with version.
                Some(version) => ToolProbe {
                    found: true,
                    version: Some(version),
                    path: Some(path.display().to_string()),
                },
                // Present but unresponsive/broken → report as missing so the UI
                // can say "未找到 / 不可用" instead of hanging on it.
                None => ToolProbe {
                    found: false,
                    version: None,
                    path: Some(path.display().to_string()),
                },
            };
        }
    }
    ToolProbe::default()
}

pub(crate) fn version_of(path: &Path, args: &[&str]) -> Option<String> {
    let mut cmd = Command::new(path);
    cmd.args(args).stdout(Stdio::piped()).stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000);
    }
    let mut child = cmd.spawn().ok()?;
    let deadline = Instant::now() + PROBE_TIMEOUT;
    let output = loop {
        match child.try_wait().ok()? {
            Some(_) => break child.wait_with_output().ok()?,
            None => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
    };
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    if text.trim().is_empty() {
        text = String::from_utf8_lossy(&output.stderr).into_owned();
    }
    let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
    let first = lines.next()?;
    // 首行可能是横幅噪音（如 `Picked up _JAVA_OPTIONS: …`），逐行找版本样式，
    // 找不到时保留原始首行（保持「已安装但版本未知」的兜底显示）。
    std::iter::once(first)
        .chain(lines)
        .find_map(extract_version)
        .or_else(|| Some(first.to_string()))
}

/// 从 `java -version` / `mvn -v` 等的输出行中提取裸版本号，去掉
/// 工具名前缀与 build 时间等括号注释：
/// - `openjdk version "17.0.10" 2024-01-16` → `17.0.10`
/// - `java version "1.8.0_392"`             → `1.8.0_392`
/// - `Apache Maven 3.9.9 (build id…)`       → `3.9.9`
/// - `v22.4.0` / `10.7.0`                    → `22.4.0` / `10.7.0`
fn extract_version(line: &str) -> Option<String> {
    let re = regex::Regex::new(r"(?i)\b(?:jre|jdk)?v?(\d+(?:\.\d+)+(?:[._-][0-9A-Za-z]+)*)").ok()?;
    re.captures(line)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
}

/// kind 启动前置工具检查。`build_tool`（1.4 §5.1）只对 spring-boot 有意义：
/// gradle 服务不需要 mvn（wrapper/gradle 的存在性由 `GRADLE_WRAPPER_MISSING` 路径负责）。
pub fn require_tools_for_kind(kind: &str, pkg: Option<&str>, build_tool: Option<&str>) -> Result<()> {
    let p = probe_toolchain();
    match kind {
        "spring-boot" => {
            if !p.java.found {
                return Err(Error::new(ErrorCode::MissingTool, "未找到 java。请安装 JDK 并确保在 PATH 中。"));
            }
            if build_tool == Some("gradle") {
                return Ok(());
            }
            if !p.maven.found {
                return Err(Error::new(ErrorCode::MissingTool, "未找到 mvn。请安装 Maven 并确保在 PATH 中，或在「环境」页一键安装。"));
            }
        }
        "node" => {
            if !p.node.found {
                return Err(Error::new(ErrorCode::MissingTool, "未找到 node。请安装 Node.js 并确保在 PATH 中。"));
            }
            let need = pkg.unwrap_or("npm");
            let ok = match need {
                "pnpm" => p.pnpm.found,
                "yarn" => p.yarn.found,
                _ => p.npm.found,
            };
            if !ok {
                return Err(Error::new(
                    ErrorCode::MissingTool,
                    format!("未找到 {need}。请安装并确保在 PATH 中。"),
                ));
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_lookup_self() {
        #[cfg(windows)]
        {
            assert!(find_on_path("cmd.exe").is_some() || find_on_path("cmd").is_some());
            assert!(find_on_path("definitely-not-a-real-tool-zxq").is_none());
        }
    }

    #[test]
    fn probe_runs() {
        let t = probe_toolchain();
        let _ = t.java.found;
    }

    #[test]
    fn extract_version_from_real_outputs() {
        // `java -version` / `mvn -v` / `node -v` 等真实首行
        assert_eq!(
            extract_version(r#"openjdk version "17.0.10" 2024-01-16"#).as_deref(),
            Some("17.0.10")
        );
        assert_eq!(
            extract_version(r#"java version "1.8.0_392""#).as_deref(),
            Some("1.8.0_392")
        );
        assert_eq!(
            extract_version("Apache Maven 3.9.9 (b2f2ec48, 2024-08-27T11:02:50Z)").as_deref(),
            Some("3.9.9")
        );
        assert_eq!(extract_version("v22.4.0").as_deref(), Some("22.4.0"));
        assert_eq!(extract_version("V10.7.0").as_deref(), Some("10.7.0"));
        assert_eq!(
            extract_version("pnpm 9.7.1, standalone").as_deref(),
            Some("9.7.1")
        );
        // 本机（2026-08）实测输出
        assert_eq!(
            extract_version(r#"java version "25.0.1" 2025-10-21 LTS"#).as_deref(),
            Some("25.0.1")
        );
        assert_eq!(
            extract_version("Apache Maven 3.9.11 (3e54c93a704957b63ee3494413a2b544fd3d825b)")
                .as_deref(),
            Some("3.9.11")
        );
        assert_eq!(extract_version("v24.19.0").as_deref(), Some("24.19.0"));
    }

    #[test]
    fn extract_version_skips_banner_noise_and_dates() {
        // 横幅噪音行不产生版本；build 日期行也不会盖掉真正的版本
        assert_eq!(extract_version("Picked up _JAVA_OPTIONS: -Xmx512m"), None);
        assert_eq!(
            version_text_first_match("Picked up _JAVA_OPTIONS: -Xmx512m\nopenjdk version \"21.0.2\" 2024-01-16").as_deref(),
            Some("21.0.2")
        );
        // 无版本样式 → 兜底保留原始首行
        assert_eq!(
            version_text_first_match("some unknown tool banner").as_deref(),
            Some("some unknown tool banner")
        );
    }

    /// 复刻 version_of 的「逐行找，找不到兜底首行」选择逻辑（测试纯文本用）。
    fn version_text_first_match(text: &str) -> Option<String> {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        let first = lines.next()?;
        std::iter::once(first)
            .chain(lines)
            .find_map(extract_version)
            .or_else(|| Some(first.to_string()))
    }
}
