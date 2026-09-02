//! Resolve installed tools. Installer exit 0 is not enough (§4.4).

use super::provider;
use super::runner::{run_mapped, ToolRunner};
use super::{ProviderKind, ToolKind};
use crate::error::{Error, ErrorCode, Result};
use crate::probe;
use indexmap::IndexMap;
use std::path::{Path, PathBuf};

/// winget 分支解析可执行文件的探测函数；生产用 `probe::find_on_path`，
/// 测试注入假探针避免依赖真机 PATH。
pub type PathProbe = fn(&str) -> Option<PathBuf>;

#[derive(Debug, Clone)]
pub struct ResolvedTool {
    pub program: PathBuf,
    pub version: Option<String>,
    pub env_delta: IndexMap<String, String>,
}

pub fn resolve_tool(
    runner: &dyn ToolRunner,
    provider: ProviderKind,
    tool: ToolKind,
    workspace: &Path,
) -> Result<ResolvedTool> {
    resolve_tool_with(runner, provider, tool, workspace, probe::find_on_path)
}

pub fn resolve_tool_with(
    runner: &dyn ToolRunner,
    provider: ProviderKind,
    tool: ToolKind,
    workspace: &Path,
    path_probe: PathProbe,
) -> Result<ResolvedTool> {
    match provider {
        ProviderKind::Mise => {
            let spec = provider::which_spec(tool, Some(workspace.to_path_buf()));
            let out = run_mapped(runner, &spec)?;
            let path = out
                .stdout
                .lines()
                .map(|s| s.trim())
                .find(|s| !s.is_empty())
                .unwrap_or("");
            if out.code != 0 || path.is_empty() {
                return Err(Error::new(
                    ErrorCode::MissingTool,
                    format!("未解析到 {}", tool.as_str()),
                ));
            }
            let mut env_delta = IndexMap::new();
            if let Some(parent) = Path::new(path).parent() {
                env_delta.insert(
                    "PATH".into(),
                    format!(
                        "{};{}",
                        parent.display(),
                        std::env::var("PATH").unwrap_or_default()
                    ),
                );
            }
            Ok(ResolvedTool {
                program: PathBuf::from(path),
                version: None,
                env_delta,
            })
        }
        ProviderKind::Winget => {
            refresh_process_path();
            for n in tool.path_names() {
                if let Some(p) = path_probe(n) {
                    return Ok(ResolvedTool {
                        program: p,
                        version: None,
                        env_delta: IndexMap::new(),
                    });
                }
            }
            Err(Error::new(
                ErrorCode::MissingTool,
                format!("未找到 {}", tool.as_str()),
            ))
        }
    }
}

/// 用户范围安装写入注册表 PATH 后，子进程继承的是本进程环境；
/// 合并注册表里最新的用户/机器 PATH，让刚装好的工具立即可见。
pub fn refresh_process_path() {
    #[cfg(not(windows))]
    return;

    #[cfg(windows)]
    {
        let user = read_reg_value(r"HKCU\Environment", "Path");
        let machine = read_reg_value(
            r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment",
            "Path",
        );
        let current = std::env::var("PATH").unwrap_or_default();
        let mut parts: Vec<String> = Vec::new();
        // Preserve the precedence of the already-running process, then append
        // newly published user/machine entries so an installer becomes visible
        // without changing an explicitly prepared application PATH.
        for src in [Some(current), user, machine] {
            let Some(src) = src else { continue };
            for p in src.split(';') {
                let p = p.trim();
                if !p.is_empty() && !parts.iter().any(|x| x.eq_ignore_ascii_case(p)) {
                    parts.push(p.to_string());
                }
            }
        }
        std::env::set_var("PATH", parts.join(";"));
    }
}

/// `reg query <key> /v <value>` 输出形如
/// `    Path    REG_EXPAND_SZ    C:\Program Files\...`。
/// 值里可能含空格，不能按空白切分：定位 `_SZ` 类型标记后取其后的整段。
/// discover（本机已装枚举）复用。
pub(crate) fn read_reg_value(key: &str, value: &str) -> Option<String> {
    let out = std::process::Command::new("reg")
        .args(["query", key, "/v", value])
        .creation_flags_no_window()
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&out.stdout);
    text.lines().find_map(|line| parse_reg_line(line, value))
}

fn parse_reg_line(line: &str, value: &str) -> Option<String> {
    let line = line.trim();
    if !line
        .get(..value.len())
        .is_some_and(|head| head.eq_ignore_ascii_case(value))
    {
        return None;
    }
    let marker = line.to_ascii_uppercase().find("_SZ")?;
    let rest = &line[marker + "_SZ".len()..];
    let v = rest.trim();
    if v.is_empty() {
        None
    } else {
        Some(v.to_string())
    }
}

trait NoWindow {
    fn creation_flags_no_window(&mut self) -> &mut Self;
}

impl NoWindow for std::process::Command {
    fn creation_flags_no_window(&mut self) -> &mut Self {
        #[cfg(windows)]
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
    fn reg_line_parse_keeps_spaces_in_value() {
        assert_eq!(
            parse_reg_line(
                "    Path    REG_EXPAND_SZ    C:\\Program Files\\Java\\bin;C:\\tools",
                "Path"
            ),
            Some("C:\\Program Files\\Java\\bin;C:\\tools".to_string())
        );
        assert_eq!(
            parse_reg_line("    ProxyServer    REG_SZ    127.0.0.1:7890", "ProxyServer"),
            Some("127.0.0.1:7890".to_string())
        );
        // 其他键的行、无类型标记的行不命中
        assert_eq!(
            parse_reg_line("    ProxyEnable    REG_DWORD    0x1", "Path"),
            None
        );
        assert_eq!(
            parse_reg_line("HKEY_CURRENT_USER\\Environment", "Path"),
            None
        );
    }

    #[test]
    fn mise_which_failure_is_missing_tool() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_fail(1, "");
        let e =
            resolve_tool(&fake, ProviderKind::Mise, ToolKind::Java, Path::new("C:/w")).unwrap_err();
        assert_eq!(e.code(), ErrorCode::MissingTool);
    }

    #[test]
    fn mise_which_returns_program_and_path_env_delta() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_ok("C:\\mise\\installs\\java\\21\\bin\\java.exe");
        let tool =
            resolve_tool(&fake, ProviderKind::Mise, ToolKind::Java, Path::new("C:/w")).unwrap();
        assert_eq!(
            tool.program,
            PathBuf::from("C:\\mise\\installs\\java\\21\\bin\\java.exe")
        );
        assert!(tool
            .env_delta
            .get("PATH")
            .unwrap()
            .starts_with("C:\\mise\\installs\\java\\21\\bin;"));
    }

    #[test]
    fn winget_probe_miss_is_missing_tool() {
        fn never(_: &str) -> Option<PathBuf> {
            None
        }
        let fake = crate::toolchain::runner::FakeRunner::new();
        let e = resolve_tool_with(
            &fake,
            ProviderKind::Winget,
            ToolKind::Java,
            Path::new("C:/w"),
            never,
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::MissingTool);
    }
}
