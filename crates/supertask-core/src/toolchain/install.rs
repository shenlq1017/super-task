//! 安装 / 升级主流程（§4.2–§4.5）。runner 可注入：测试用 `FakeRunner`，
//! 壳层传 `ProcessRunner`。
//!
//! 流程：校验版本 → 探测 provider 可用性 → auto 选择 → 固定 argv 安装 →
//! 失败分类（权限 / 一般）→ 成功后立即重新解析（解析失败 `MISSING_TOOL`，
//! 不接受「安装命令返回 0」作为工具可用的唯一依据）。

use std::path::Path;

use indexmap::IndexMap;

use super::provider;
use super::resolver::{self, ResolvedTool};
use super::runner::{run_mapped, ToolOutput, ToolRunner};
use super::{ProviderKind, ToolKind};
use crate::error::{Error, ErrorCode, Result};
use crate::spec::ToolchainManager;

#[derive(Debug, Clone)]
pub struct InstallOutcome {
    pub tool: ToolKind,
    pub version: String,
    pub manager: ProviderKind,
    pub resolved: ResolvedTool,
}

pub struct InstallRequest<'a> {
    pub tool: ToolKind,
    /// 已解析的版本；调用方负责在缺省时填 `manifest::default_version`。
    pub version: &'a str,
    /// 用户在安装页面临时选择的 manager；`Some(Auto)` 表示强制自动选择。
    pub requested: Option<ToolchainManager>,
    /// 工作区 `toolchain.manager`（auto 选择时优先于内置顺序）。
    pub workspace_manager: Option<ToolchainManager>,
    /// mise 解析用的项目根目录；无工作区时可传任意已存在目录。
    pub workspace: &'a Path,
    /// 代理等附加环境（来自 network 策略）；只注入已校验的键值。
    pub env: IndexMap<String, String>,
    /// winget 分支解析可执行文件的 PATH 探测；测试注入假探针。
    pub path_probe: resolver::PathProbe,
}

pub fn install(runner: &dyn ToolRunner, req: InstallRequest<'_>) -> Result<InstallOutcome> {
    run_flow(runner, req, false)
}

pub fn upgrade(runner: &dyn ToolRunner, req: InstallRequest<'_>) -> Result<InstallOutcome> {
    run_flow(runner, req, true)
}

fn run_flow(
    runner: &dyn ToolRunner,
    req: InstallRequest<'_>,
    upgrade: bool,
) -> Result<InstallOutcome> {
    validate_version(req.version)?;
    let available = provider::detect_availability(runner);
    let manager = provider::select_manager(req.requested, req.workspace_manager, available)?;

    let spec = if upgrade {
        provider::upgrade_spec(manager, req.tool, req.version, req.env.clone())
    } else {
        provider::install_spec(manager, req.tool, req.version, req.env.clone())
    }?;
    let output = run_mapped(runner, &spec)?;
    if output.code != 0 {
        return Err(install_failure(&output, req.tool, manager, upgrade));
    }

    // 安装成功 ≠ 工具可用：立即重新解析（§4.4）。winget 分支内部会刷新进程 PATH。
    let resolved =
        resolver::resolve_tool_with(runner, manager, req.tool, req.workspace, req.path_probe)?;
    Ok(InstallOutcome {
        tool: req.tool,
        version: req.version.to_string(),
        manager,
        resolved,
    })
}

/// 失败保留已有工具与原 YAML（§4.5）：这里只报错，不做任何删除/回滚。
/// 消息只保留 stderr 尾行摘要，完整输出按本地诊断策略处理，不进事件流。
fn install_failure(
    output: &ToolOutput,
    tool: ToolKind,
    manager: ProviderKind,
    upgrade: bool,
) -> Error {
    let verb = if upgrade { "升级" } else { "安装" };
    let code = provider::classify_output(output);
    let hint = match code {
        ErrorCode::ToolchainPermission => "provider 需要管理员权限，请到系统安装器完成",
        _ => "已保留原有工具与配置",
    };
    let tail = output
        .stderr
        .lines()
        .rev()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("provider 未输出原因");
    let tail: String = tail.chars().take(200).collect();
    Error::new(
        code,
        format!(
            "{verb} {}（{}）失败: {tail}。{hint}",
            tool.as_str(),
            manager.as_str()
        ),
    )
}

/// `tool` 只接受规格 §13.1 的六个逻辑名。
pub fn parse_tool(name: &str) -> Result<ToolKind> {
    ToolKind::parse(name).ok_or_else(|| {
        Error::new(
            ErrorCode::SpecInvalid,
            format!("tool 仅接受 java|maven|node|npm|pnpm|yarn，收到 {name:?}"),
        )
    })
}

/// 版本输入（§4.3）：复用 YAML 侧字符集校验，额外禁止前导 `-`，
/// 防止版本被当作 provider 的 argv 开关。
pub fn validate_version(s: &str) -> Result<()> {
    if s.starts_with('-') {
        return Err(Error::new(
            ErrorCode::ToolchainVersionInvalid,
            "版本不能以 - 开头",
        ));
    }
    if !crate::spec::validate::is_valid_toolchain_version(s) {
        return Err(Error::new(
            ErrorCode::ToolchainVersionInvalid,
            format!("非法版本 {s:?}: 只允许数字、点号、连字符与 lts 别名"),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::ToolchainManager;
    use std::path::PathBuf;

    fn req<'a>(
        tool: ToolKind,
        version: &'a str,
        requested: Option<ToolchainManager>,
        ws_manager: Option<ToolchainManager>,
        workspace: &'a Path,
    ) -> InstallRequest<'a> {
        InstallRequest {
            tool,
            version,
            requested,
            workspace_manager: ws_manager,
            workspace,
            env: IndexMap::new(),
            path_probe: fake_probe,
        }
    }

    /// winget 分支不走 runner，用假探针保证测试不依赖真机 PATH。
    fn fake_probe(name: &str) -> Option<PathBuf> {
        Some(PathBuf::from(format!("C:\\fake\\bin\\{name}")))
    }

    /// script 依次喂给 FakeRunner：mise --version → winget --version → install → mise which。
    fn mise_happy_script(fake: &crate::toolchain::runner::FakeRunner) {
        fake.push_ok("mise 2024.1"); // mise --version
        fake.push_ok("winget v1.6"); // winget --version
        fake.push_ok("installed java@21"); // mise install
        fake.push_ok("C:\\tools\\mise\\java\\21\\bin\\java.exe"); // mise which
    }

    #[test]
    fn auto_prefers_mise_and_uses_fixed_argv() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        mise_happy_script(&fake);
        let ws = PathBuf::from("C:/work/mall");
        let out = install(&fake, req(ToolKind::Java, "21", None, None, &ws)).unwrap();
        assert_eq!(out.manager, ProviderKind::Mise);
        assert_eq!(
            out.resolved.program,
            PathBuf::from("C:\\tools\\mise\\java\\21\\bin\\java.exe")
        );
        let calls = fake.calls();
        // 安装 argv 固定：`mise install java@21`，无任何拼接 shell
        assert_eq!(calls[2].program, "mise");
        assert_eq!(
            calls[2].args,
            vec!["install".to_string(), "java@21".to_string()]
        );
    }

    #[test]
    fn auto_falls_back_to_winget_when_mise_missing() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_fail(9009, "mise not found"); // mise --version → io 层为 exit code 模拟
        fake.push_ok("winget v1.6"); // winget --version
        fake.push_ok("installed"); // winget install
        let ws = PathBuf::from("C:/work/mall");
        let out = install(&fake, req(ToolKind::Node, "20", None, None, &ws)).unwrap();
        assert_eq!(out.manager, ProviderKind::Winget);
        let calls = fake.calls();
        let argv = calls[2].args.clone();
        // 包 ID 来自 manifest，版本与工具名不进 argv；--scope user，无提权参数
        assert!(argv
            .windows(2)
            .any(|w| w == ["--id".to_string(), "OpenJS.NodeJS.LTS".to_string()]));
        assert!(argv
            .windows(2)
            .any(|w| w == ["--scope".to_string(), "user".to_string()]));
        assert!(argv.iter().all(|a| !a.contains("admin")));
    }

    #[test]
    fn explicit_manager_overrides_auto() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_ok("mise 2024.1");
        fake.push_ok("winget v1.6");
        fake.push_ok("installed");
        fake.push_ok("C:\\tools\\mvn\\bin\\mvn.cmd");
        let ws = PathBuf::from("C:/work/mall");
        let out = install(
            &fake,
            req(
                ToolKind::Maven,
                "3.9",
                Some(ToolchainManager::Winget),
                None,
                &ws,
            ),
        )
        .unwrap();
        assert_eq!(out.manager, ProviderKind::Winget);
    }

    #[test]
    fn workspace_manager_honored_over_builtin_order() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        // 两者都可用，但工作区固定 winget → 应选 winget 而非 mise
        fake.push_ok("mise 2024.1");
        fake.push_ok("winget v1.6");
        fake.push_ok("installed");
        fake.push_ok("C:\\tools\\node\\node.exe");
        let ws = PathBuf::from("C:/work/mall");
        let out = install(
            &fake,
            req(
                ToolKind::Node,
                "20",
                None,
                Some(ToolchainManager::Winget),
                &ws,
            ),
        )
        .unwrap();
        assert_eq!(out.manager, ProviderKind::Winget);
    }

    #[test]
    fn missing_managers_is_manager_missing() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_fail(9009, "no mise");
        fake.push_fail(9009, "no winget");
        let ws = PathBuf::from("C:/work/mall");
        let e = install(&fake, req(ToolKind::Java, "21", None, None, &ws)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ToolchainManagerMissing);
    }

    #[test]
    fn permission_failure_classified_and_nothing_deleted() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_ok("mise 2024.1");
        fake.push_ok("winget v1.6");
        fake.push_fail(1, "installation requires administrator rights");
        let ws = PathBuf::from("C:/work/mall");
        let e = install(&fake, req(ToolKind::Java, "21", None, None, &ws)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::ToolchainPermission);
        // 失败路径只发生 2 次探测 + 1 次安装调用，没有后续删除/解析
        assert_eq!(fake.calls().len(), 3);
    }

    #[test]
    fn install_exit_zero_but_resolve_fails_is_missing_tool() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        fake.push_ok("mise 2024.1");
        fake.push_ok("winget v1.6");
        fake.push_ok("installed java@21");
        fake.push_fail(1, ""); // mise which → 空结果
        let ws = PathBuf::from("C:/work/mall");
        let e = install(&fake, req(ToolKind::Java, "21", None, None, &ws)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::MissingTool);
    }

    #[test]
    fn invalid_versions_rejected_before_any_spawn() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        let ws = PathBuf::from("C:/work/mall");
        for bad in ["", "-21", "21;rm", "a b", "../../../etc"] {
            let e = install(&fake, req(ToolKind::Java, bad, None, None, &ws)).unwrap_err();
            assert_eq!(e.code(), ErrorCode::ToolchainVersionInvalid, "{bad}");
        }
        assert!(fake.calls().is_empty(), "非法版本不允许产生任何 spawn");
    }

    #[test]
    fn upgrade_uses_upgrade_subcommand() {
        let fake = crate::toolchain::runner::FakeRunner::new();
        mise_happy_script(&fake);
        let ws = PathBuf::from("C:/work/mall");
        let out = upgrade(&fake, req(ToolKind::Java, "21", None, None, &ws)).unwrap();
        assert_eq!(out.manager, ProviderKind::Mise);
        let calls = fake.calls();
        assert_eq!(
            calls[2].args,
            vec!["upgrade".to_string(), "java@21".to_string()]
        );
    }
}
