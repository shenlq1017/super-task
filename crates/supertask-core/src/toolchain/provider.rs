//! Provider auto-select and fixed argv builders.

use super::manifest;
use super::runner::SpawnSpec;
use super::{ProviderKind, ToolKind};
use crate::error::{Error, ErrorCode, Result};
use crate::spec::ToolchainManager;
use indexmap::IndexMap;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, serde::Serialize)]
pub struct ManagerAvailability {
    pub mise: bool,
    pub winget: bool,
}

pub fn select_manager(
    requested: Option<ToolchainManager>,
    workspace: Option<ToolchainManager>,
    available: ManagerAvailability,
) -> Result<ProviderKind> {
    let pref = match requested {
        Some(ToolchainManager::Mise) | Some(ToolchainManager::Winget) => requested.unwrap(),
        Some(ToolchainManager::Auto) | None => match workspace {
            Some(ToolchainManager::Mise) | Some(ToolchainManager::Winget) => workspace.unwrap(),
            _ => ToolchainManager::Auto,
        },
    };
    match pref {
        ToolchainManager::Mise => {
            if available.mise {
                Ok(ProviderKind::Mise)
            } else {
                Err(Error::new(
                    ErrorCode::ToolchainManagerMissing,
                    "未找到 mise。请安装后重试。",
                ))
            }
        }
        ToolchainManager::Winget => {
            if available.winget {
                Ok(ProviderKind::Winget)
            } else {
                Err(Error::new(
                    ErrorCode::ToolchainManagerMissing,
                    "未找到 winget。请安装后重试。",
                ))
            }
        }
        ToolchainManager::Auto => {
            if available.mise {
                Ok(ProviderKind::Mise)
            } else if available.winget {
                Ok(ProviderKind::Winget)
            } else {
                Err(Error::new(
                    ErrorCode::ToolchainManagerMissing,
                    "mise 和 winget 都不可用",
                ))
            }
        }
    }
}

const INSTALL_TIMEOUT: Duration = Duration::from_secs(15 * 60);
const PROBE_TIMEOUT: Duration = Duration::from_secs(4);

pub fn version_probe_spec(program: &str) -> SpawnSpec {
    SpawnSpec {
        program: program.to_string(),
        args: vec!["--version".into()],
        cwd: None,
        env: IndexMap::new(),
        timeout: PROBE_TIMEOUT,
    }
}

pub fn detect_availability(runner: &dyn super::runner::ToolRunner) -> ManagerAvailability {
    let run = |prog: &str| -> bool {
        runner
            .run(&version_probe_spec(prog))
            .map(|o| o.code == 0)
            .unwrap_or(false)
    };
    ManagerAvailability {
        mise: run("mise"),
        winget: run("winget"),
    }
}

pub fn install_spec(
    provider: ProviderKind,
    tool: ToolKind,
    version: &str,
    env: IndexMap<String, String>,
) -> Result<SpawnSpec> {
    argv(provider, tool, version, false, env)
}
pub fn upgrade_spec(
    provider: ProviderKind,
    tool: ToolKind,
    version: &str,
    env: IndexMap<String, String>,
) -> Result<SpawnSpec> {
    argv(provider, tool, version, true, env)
}

fn argv(
    provider: ProviderKind,
    tool: ToolKind,
    version: &str,
    upgrade: bool,
    env: IndexMap<String, String>,
) -> Result<SpawnSpec> {
    match provider {
        ProviderKind::Mise => {
            let logical = format!("{}@{}", manifest::mise_tool_name(tool), version);
            let cmd = if upgrade { "upgrade" } else { "install" }.to_string();
            Ok(SpawnSpec {
                program: "mise".into(),
                args: vec![cmd, logical],
                cwd: None,
                env,
                timeout: INSTALL_TIMEOUT,
            })
        }
        ProviderKind::Winget => {
            let id = manifest::winget_id(tool, version)?;
            let cmd = if upgrade { "upgrade" } else { "install" }.to_string();
            Ok(SpawnSpec {
                program: "winget".into(),
                args: vec![
                    cmd,
                    "--id".into(),
                    id.to_string(),
                    "--accept-package-agreements".into(),
                    "--accept-source-agreements".into(),
                    "--disable-interactivity".into(),
                    "--scope".into(),
                    "user".into(),
                ],
                cwd: None,
                env,
                timeout: INSTALL_TIMEOUT,
            })
        }
    }
}

pub fn which_spec(tool: ToolKind, cwd: Option<std::path::PathBuf>) -> SpawnSpec {
    SpawnSpec {
        program: "mise".into(),
        args: vec!["which".into(), manifest::mise_tool_name(tool).into()],
        cwd,
        env: IndexMap::new(),
        timeout: PROBE_TIMEOUT,
    }
}

pub fn classify_output(output: &super::runner::ToolOutput) -> ErrorCode {
    let text = format!("{}\n{}", output.stderr, output.stdout).to_ascii_lowercase();
    if text.contains("administrator")
        || text.contains("elevat")
        || text.contains("access is denied")
        || text.contains("permission denied")
        || text.contains("requires admin")
        || text.contains("uac")
    {
        ErrorCode::ToolchainPermission
    } else {
        ErrorCode::ToolchainInstallFailed
    }
}

pub fn is_destructive_arg(arg: &str) -> bool {
    let a = arg.to_ascii_lowercase();
    a == "uninstall" || a == "remove" || a == "purge" || a == "--uninstall"
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ToolchainManager;

    #[test]
    fn auto_order_mise_then_winget() {
        let both = ManagerAvailability {
            mise: true,
            winget: true,
        };
        let only_winget = ManagerAvailability {
            mise: false,
            winget: true,
        };
        let none = ManagerAvailability {
            mise: false,
            winget: false,
        };
        // auto：mise 可用优先
        assert_eq!(
            select_manager(None, None, both).unwrap(),
            ProviderKind::Mise
        );
        // 工作区固定 manager 优先于内置顺序
        assert_eq!(
            select_manager(None, Some(ToolchainManager::Winget), both).unwrap(),
            ProviderKind::Winget
        );
        // 用户临时选择优先于工作区
        assert_eq!(
            select_manager(
                Some(ToolchainManager::Mise),
                Some(ToolchainManager::Winget),
                both
            )
            .unwrap(),
            ProviderKind::Mise
        );
        // mise 缺失 → winget；都缺失 → TOOLCHAIN_MANAGER_MISSING
        assert_eq!(
            select_manager(None, None, only_winget).unwrap(),
            ProviderKind::Winget
        );
        assert_eq!(
            select_manager(None, None, none).unwrap_err().code(),
            ErrorCode::ToolchainManagerMissing
        );
        // 指定 mise 但不可用 → 快速失败，不静默换 provider
        assert_eq!(
            select_manager(Some(ToolchainManager::Mise), None, only_winget)
                .unwrap_err()
                .code(),
            ErrorCode::ToolchainManagerMissing
        );
    }

    #[test]
    fn winget_argv_never_carries_user_version() {
        let env = IndexMap::new();
        let spec = install_spec(ProviderKind::Winget, ToolKind::Java, "999", env).unwrap_err();
        assert_eq!(spec.code(), ErrorCode::ToolchainVersionInvalid);
    }

    #[test]
    fn classify_maps_permission_errors() {
        let out = crate::toolchain::runner::ToolOutput {
            code: 1,
            stdout: String::new(),
            stderr: "access is denied".into(),
        };
        assert_eq!(classify_output(&out), ErrorCode::ToolchainPermission);
        let out = crate::toolchain::runner::ToolOutput {
            code: 1,
            stdout: String::new(),
            stderr: "download failed: 404".into(),
        };
        assert_eq!(classify_output(&out), ErrorCode::ToolchainInstallFailed);
    }
}
