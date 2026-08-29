//! 1.2 工具链安装与解析。Spec: `docs/plans/2026-08-27-v1-2-feature-spec.md` §4。
//!
//! 硬性边界：
//! - 安装 provider 只用固定程序名 + 结构化 argv（见 [`runner`]），禁止拼接 shell 字符串；
//! - winget 包 ID 来自应用内置 manifest，绝不由 YAML / UI 传入；
//! - 默认不请求管理员；provider 要求提权时快速失败 `TOOLCHAIN_PERMISSION`；
//! - 安装命令返回 0 不代表工具可用，必须经 [`resolver`] 解析成功。

pub mod install;
pub mod manifest;
pub mod provider;
pub mod resolver;
pub mod runner;

pub use install::{install, parse_tool, upgrade, validate_version, InstallOutcome, InstallRequest};
pub use provider::ManagerAvailability;
pub use runner::{ProcessRunner, ToolRunner};

/// 逻辑工具名（§4.1 / §13.1）。npm/pnpm/yarn 是包管理器，不是独立语言运行时。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolKind {
    Java,
    Maven,
    Node,
    Npm,
    Pnpm,
    Yarn,
    /// 1.7
    Python,
    /// 1.7
    Go,
}

impl ToolKind {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "java" => Some(Self::Java),
            "maven" => Some(Self::Maven),
            "node" => Some(Self::Node),
            "npm" => Some(Self::Npm),
            "pnpm" => Some(Self::Pnpm),
            "yarn" => Some(Self::Yarn),
            "python" => Some(Self::Python),
            "go" => Some(Self::Go),
            _ => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Java => "java",
            Self::Maven => "maven",
            Self::Node => "node",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Yarn => "yarn",
            Self::Python => "python",
            Self::Go => "go",
        }
    }

    /// winget 用户范围安装后，按命中顺序探测的可执行名（含 PATHEXT 后缀）。
    pub fn path_names(self) -> &'static [&'static str] {
        match self {
            Self::Java => &["java.exe", "java"],
            Self::Maven => &["mvn.cmd", "mvn.bat", "mvn.exe", "mvn"],
            Self::Node => &["node.exe", "node"],
            Self::Npm => &["npm.cmd", "npm.exe", "npm"],
            Self::Pnpm => &["pnpm.cmd", "pnpm.exe", "pnpm"],
            Self::Yarn => &["yarn.cmd", "yarn.exe", "yarn"],
            Self::Python => &["python.exe", "python", "python3"],
            Self::Go => &["go.exe", "go"],
        }
    }
}

/// 安装 provider（§4.2）。`auto` 顺序在 [`provider::select_manager`]。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderKind {
    Mise,
    Winget,
}

impl ProviderKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Mise => "mise",
            Self::Winget => "winget",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_names_round_trip() {
        for name in ["java", "maven", "node", "npm", "pnpm", "yarn"] {
            assert_eq!(ToolKind::parse(name).unwrap().as_str(), name);
        }
        assert!(ToolKind::parse("rm").is_none());
        assert!(ToolKind::parse("java ").is_none());
    }
}
