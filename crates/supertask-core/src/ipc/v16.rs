//! 1.6 IPC 契约增量（ipc.md §10.10）：gateway.* 命令的数据结构。
//! protocol 保持 1；`gateway.start/stop/restart/trust` 复用 `OkOutput`。

use serde::{Deserialize, Serialize};

/// 状态视图里的一条路由（含解析出的目标端口与上游存活）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayRouteView {
    pub host: Option<String>,
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_port: Option<u16>,
    /// 上游端口是否在本机监听（loopback 双栈探测）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub upstream_alive: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayStatusOutput {
    pub configured: bool,
    pub enabled: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    /// 未配置时为 None（前端空态用）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub port: Option<u16>,
    /// starting | running | unhealthy | stopped | stopping | exited
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
    pub routes: Vec<GatewayRouteView>,
    /// 生成物绝对路径（已落盘时返回）
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub conf_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayFileView {
    pub name: String,
    pub content: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayPreviewOutput {
    pub files: Vec<GatewayFileView>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GatewayValidateOutput {
    pub ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stderr: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayApplyInput {
    pub workspace_id: String,
    pub gateway: crate::spec::GatewayConf,
    pub base_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayApplyOutput {
    pub spec: serde_yaml::Value,
    pub hash: String,
    pub restarted: bool,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayPreviewInput {
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<crate::spec::GatewayConf>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GatewayValidateInput {
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub gateway: Option<crate::spec::GatewayConf>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_output_field_names_are_snake_case() {
        let out = GatewayStatusOutput {
            configured: true,
            enabled: true,
            kind: Some("nginx".into()),
            port: Some(8080),
            state: Some("running".into()),
            pid: Some(42),
            last_error: None,
            routes: vec![GatewayRouteView {
                host: None,
                path: "/api".into(),
                target: Some("user-api".into()),
                upstream: None,
                target_port: Some(8081),
                upstream_alive: Some(true),
            }],
            conf_path: None,
        };
        let text = serde_yaml::to_string(&out).unwrap();
        assert!(text.contains("target_port"));
        assert!(text.contains("upstream_alive"));
        assert!(text.contains("kind: nginx"));
        assert!(!text.contains("last_error"), "None 字段缺省不序列化");
    }

    #[test]
    fn apply_output_round_trip() {
        let out = GatewayApplyOutput {
            spec: serde_yaml::Value::Null,
            hash: "abc".into(),
            restarted: true,
            warnings: vec!["w".into()],
        };
        let text = serde_yaml::to_string(&out).unwrap();
        assert!(text.contains("restarted: true"));
        let back: GatewayApplyOutput = serde_yaml::from_str(&text).unwrap();
        assert_eq!(back.hash, "abc");
    }
}
