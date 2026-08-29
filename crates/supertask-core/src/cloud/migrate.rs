//! 一键迁移（v2.0 规格 §8）：实体清单 + 工具链差量 → 落盘 + 显式安装。
//! 安装复用 1.2 mise/winget 链（`toolchain::install`），**不代装原则不变**——
//! plan 只给差量清单，apply 由用户显式触发。

use serde::{Deserialize, Serialize};

use crate::probe::ToolchainProbe;
use crate::spec::ToolchainSpec;

/// 单个工具链差量项。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "status")]
pub enum ToolchainGap {
    /// 已装且钉扎匹配 → 无需动作。
    Ok { tool: String },
    /// 未探测到 → 可一键安装（钉扎版本或 manifest 缺省版本）。
    Missing { tool: String, version: String },
    /// 已装但版本与钉扎不符 → 仅 warning，不自动升降级。
    VersionMismatch {
        tool: String,
        required: String,
        found: String,
    },
    /// 未钉扎 → 跳过。
    Unpinned { tool: String },
}

/// 恢复计划（`cloud.migrate.plan` 出参）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestorePlan {
    /// 工具链差量（全部工作区钉扎的并集去重）。
    pub toolchain_gaps: Vec<ToolchainGap>,
}

/// 对一个工作区的 ToolchainSpec 钉扎 × 当前 probe 比对（spec §8 步骤 5）。
pub fn toolchain_gaps(spec: &ToolchainSpec, probe: &ToolchainProbe) -> Vec<ToolchainGap> {
    fn one(
        tool: &str,
        pinned: Option<&str>,
        found: bool,
        version: Option<&str>,
        default: &str,
    ) -> Option<ToolchainGap> {
        let Some(pin) = pinned else {
            // 未钉扎：跳过（零噪音——除非探测也缺失才提示）
            return if found {
                None
            } else {
                Some(ToolchainGap::Unpinned { tool: tool.into() })
            };
        };
        if !found {
            return Some(ToolchainGap::Missing {
                tool: tool.into(),
                version: if pin.eq_ignore_ascii_case("lts") {
                    default.into()
                } else {
                    pin.into()
                },
            });
        }
        match version {
            Some(v) if !pin_matches(pin, v) => Some(ToolchainGap::VersionMismatch {
                tool: tool.into(),
                required: pin.into(),
                found: v.into(),
            }),
            _ => Some(ToolchainGap::Ok { tool: tool.into() }),
        }
    }
    let mut gaps = Vec::new();
    for (tool, pin, tp, default) in [
        (
            "java",
            spec.java.as_deref(),
            &probe.java,
            crate::toolchain::manifest::DEFAULT_JAVA,
        ),
        (
            "maven",
            spec.maven.as_deref(),
            &probe.maven,
            crate::toolchain::manifest::DEFAULT_MAVEN,
        ),
        (
            "node",
            spec.node.as_deref(),
            &probe.node,
            crate::toolchain::manifest::DEFAULT_NODE,
        ),
        (
            "python",
            spec.python.as_deref(),
            &probe.python,
            crate::toolchain::manifest::DEFAULT_PYTHON,
        ),
        (
            "go",
            spec.go.as_deref(),
            &probe.go,
            crate::toolchain::manifest::DEFAULT_GO,
        ),
    ] {
        if let Some(gap) = one(tool, pin, tp.found, tp.version.as_deref(), default) {
            gaps.push(gap);
        }
    }
    gaps
}

/// 钉扎版本匹配：major.minor 前缀口径（"3.12" 命中 "3.12.4"；"21" 命中 "21.0.1"）。
fn pin_matches(pin: &str, found: &str) -> bool {
    found == pin || found.starts_with(&format!("{pin}.")) || pin.eq_ignore_ascii_case("lts")
}

/// 应用计划：安装请求清单（供调用方逐项走既有 `toolchain::install` 链）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstallRequestItem {
    pub tool: String,
    pub version: String,
}

pub fn install_requests(gaps: &[ToolchainGap]) -> Vec<InstallRequestItem> {
    gaps.iter()
        .filter_map(|g| match g {
            ToolchainGap::Missing { tool, version } => Some(InstallRequestItem {
                tool: tool.clone(),
                version: version.clone(),
            }),
            _ => None,
        })
        .collect()
}

/// 迁移编排（spec §8 步骤 4–5）：apply 由命令层驱动——逐实体落盘（sync 适配器）
/// + 逐工具安装（复用 toolchain/install），进度复用既有 operation 事件桥。
/// 本模块只产出纯数据与编排顺序，不直接执行安装（可测试性 + 取消语义在命令层）。
pub fn apply_order(gaps: &[ToolchainGap]) -> (Vec<InstallRequestItem>, usize) {
    let installs = install_requests(gaps);
    let skipped = gaps.len() - installs.len();
    (installs, skipped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::probe::ToolProbe;

    fn probe(found: bool, version: &str) -> ToolProbe {
        ToolProbe {
            found,
            version: if found { Some(version.into()) } else { None },
            path: None,
        }
    }

    #[test]
    fn gap_matrix_missing_mismatch_unpinned_ok() {
        let spec = ToolchainSpec {
            java: Some("21".into()),
            maven: Some("3.9".into()),
            node: Some("20".into()),
            python: Some("3.12".into()),
            go: None,
            ..Default::default()
        };
        let probe = ToolchainProbe {
            java: probe(true, "21.0.2"),   // 前缀命中 → Ok
            maven: probe(false, ""),       // 缺失 → Missing
            node: probe(true, "18.20.0"),  // 版本不符 → VersionMismatch
            python: probe(true, "3.13.1"), // 钉 3.12 → VersionMismatch
            npm: probe(false, ""),
            pnpm: probe(false, ""),
            yarn: probe(false, ""),
            gradle: probe(false, ""),
            ..Default::default()
        };
        let gaps = toolchain_gaps(&spec, &probe);
        assert!(
            gaps.contains(&ToolchainGap::Ok {
                tool: "java".into()
            }),
            "{gaps:?}"
        );
        assert!(gaps.contains(&ToolchainGap::Missing {
            tool: "maven".into(),
            version: "3.9".into()
        }));
        assert!(gaps.contains(&ToolchainGap::VersionMismatch {
            tool: "node".into(),
            required: "20".into(),
            found: "18.20.0".into()
        }));
        assert!(gaps.contains(&ToolchainGap::VersionMismatch {
            tool: "python".into(),
            required: "3.12".into(),
            found: "3.13.1".into()
        }));
        // go 未钉扎但 probe 缺失 → Unpinned（不提示安装）
        assert!(gaps.contains(&ToolchainGap::Unpinned { tool: "go".into() }));
        // 安装请求只含 Missing
        let (installs, skipped) = apply_order(&gaps);
        assert_eq!(
            installs,
            vec![InstallRequestItem {
                tool: "maven".into(),
                version: "3.9".into()
            }]
        );
        assert_eq!(skipped, gaps.len() - 1);
    }
}
