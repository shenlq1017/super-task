//! Versioned in-app provider manifest. Package IDs are never taken from YAML or UI.

use super::ToolKind;
use crate::error::{Error, ErrorCode, Result};

pub const WINGET_MANIFEST_VERSION: u32 = 1;

pub const DEFAULT_JAVA: &str = "21";
pub const DEFAULT_MAVEN: &str = "3.9";
pub const DEFAULT_NODE: &str = "20";
pub const DEFAULT_PNPM: &str = "9";
pub const DEFAULT_YARN: &str = "1";
/// 1.7 §5：python/go 缺省钉扎口径 major.minor。
pub const DEFAULT_PYTHON: &str = "3.12";
pub const DEFAULT_GO: &str = "1.23";

pub fn default_version(tool: ToolKind) -> &'static str {
    match tool {
        ToolKind::Java => DEFAULT_JAVA,
        ToolKind::Maven => DEFAULT_MAVEN,
        // npm 随 Node 提供（mise 装 node，winget 装 NodeJS LTS），版本跟随 Node
        ToolKind::Node | ToolKind::Npm => DEFAULT_NODE,
        ToolKind::Pnpm => DEFAULT_PNPM,
        ToolKind::Yarn => DEFAULT_YARN,
        ToolKind::Python => DEFAULT_PYTHON,
        ToolKind::Go => DEFAULT_GO,
    }
}

pub fn mise_tool_name(tool: ToolKind) -> &'static str {
    match tool {
        ToolKind::Java => "java",
        ToolKind::Maven => "maven",
        ToolKind::Node | ToolKind::Npm => "node",
        ToolKind::Pnpm => "pnpm",
        ToolKind::Yarn => "yarn",
        ToolKind::Python => "python",
        ToolKind::Go => "go",
    }
}

const WINGET_PACKAGES: &[(ToolKind, &str, &str)] = &[
    (ToolKind::Java, "21", "EclipseAdoptium.Temurin.21.JDK"),
    (ToolKind::Java, "17", "EclipseAdoptium.Temurin.17.JDK"),
    (ToolKind::Java, "11", "EclipseAdoptium.Temurin.11.JDK"),
    (ToolKind::Java, "lts", "EclipseAdoptium.Temurin.21.JDK"),
    (ToolKind::Maven, "3.9", "Apache.Maven"),
    (ToolKind::Maven, "3", "Apache.Maven"),
    (ToolKind::Maven, "lts", "Apache.Maven"),
    (ToolKind::Node, "20", "OpenJS.NodeJS.LTS"),
    (ToolKind::Node, "22", "OpenJS.NodeJS.22"),
    (ToolKind::Node, "18", "OpenJS.NodeJS.18"),
    (ToolKind::Node, "lts", "OpenJS.NodeJS.LTS"),
    (ToolKind::Npm, "20", "OpenJS.NodeJS.LTS"),
    (ToolKind::Npm, "lts", "OpenJS.NodeJS.LTS"),
    (ToolKind::Pnpm, "9", "pnpm.pnpm"),
    (ToolKind::Pnpm, "10", "pnpm.pnpm"),
    (ToolKind::Pnpm, "lts", "pnpm.pnpm"),
    (ToolKind::Yarn, "1", "Yarn.Yarn"),
    (ToolKind::Yarn, "lts", "Yarn.Yarn"),
    // 1.7 §5：python / go
    (ToolKind::Python, "3.13", "Python.Python.3.13"),
    (ToolKind::Python, "3.12", "Python.Python.3.12"),
    (ToolKind::Python, "3.11", "Python.Python.3.11"),
    (ToolKind::Python, "lts", "Python.Python.3.12"),
    (ToolKind::Go, "1.23", "GoLang.Go"),
    (ToolKind::Go, "1.22", "GoLang.Go"),
    (ToolKind::Go, "lts", "GoLang.Go"),
];

pub fn winget_id(tool: ToolKind, logical_version: &str) -> Result<&'static str> {
    let key = logical_version.trim();
    if let Some((_, _, id)) = WINGET_PACKAGES
        .iter()
        .find(|(t, v, _)| *t == tool && v.eq_ignore_ascii_case(key))
    {
        return Ok(id);
    }
    let mut prefix = key;
    while let Some((head, _)) = prefix.rsplit_once('.') {
        if let Some((_, _, id)) = WINGET_PACKAGES
            .iter()
            .find(|(t, v, _)| *t == tool && *v == head)
        {
            return Ok(id);
        }
        prefix = head;
    }
    Err(Error::new(
        ErrorCode::ToolchainVersionInvalid,
        format!(
            "winget 清单不含 {} {}（manifest v{WINGET_MANIFEST_VERSION}）",
            tool.as_str(),
            logical_version
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn ids_are_fixed_not_user_input() {
        assert_eq!(
            winget_id(ToolKind::Java, "21").unwrap(),
            "EclipseAdoptium.Temurin.21.JDK"
        );
        assert_eq!(winget_id(ToolKind::Maven, "3.9").unwrap(), "Apache.Maven");
        assert_eq!(
            winget_id(ToolKind::Java, "99").unwrap_err().code(),
            ErrorCode::ToolchainVersionInvalid
        );
    }

    #[test]
    fn v17_python_go_mappings() {
        assert_eq!(mise_tool_name(ToolKind::Python), "python");
        assert_eq!(mise_tool_name(ToolKind::Go), "go");
        // major.minor 完整命中与前缀回退（3.12.4 → 3.12）
        assert_eq!(
            winget_id(ToolKind::Python, "3.12").unwrap(),
            "Python.Python.3.12"
        );
        assert_eq!(
            winget_id(ToolKind::Python, "3.12.4").unwrap(),
            "Python.Python.3.12"
        );
        assert_eq!(winget_id(ToolKind::Go, "1.23.1").unwrap(), "GoLang.Go");
        assert_eq!(
            winget_id(ToolKind::Go, "2.0").unwrap_err().code(),
            ErrorCode::ToolchainVersionInvalid
        );
        assert_eq!(default_version(ToolKind::Python), "3.12");
        assert_eq!(default_version(ToolKind::Go), "1.23");
    }
}
