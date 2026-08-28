//! 1.2 IPC data types. Command handlers land in later phases. Protocol stays 1.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ServiceMetrics {
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub process_count: Option<u32>,
    pub sampled_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsPayload {
    pub services: IndexMap<String, Option<ServiceMetrics>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsEvent {
    pub protocol: u32,
    pub event: String,
    pub workspace_id: String,
    pub ts_ms: u64,
    pub payload: MetricsPayload,
}

impl MetricsEvent {
    pub fn new(workspace_id: impl Into<String>, ts_ms: u64, payload: MetricsPayload) -> Self {
        Self {
            protocol: super::PROTOCOL,
            event: super::event::METRICS.to_string(),
            workspace_id: workspace_id.into(),
            ts_ms,
            payload,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OperationIdOutput {
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkspaceIdInput {
    pub workspace_id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OkOutput {
    pub ok: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolchainInstallInput {
    pub tool: String,
    pub version: Option<String>,
    pub manager: Option<String>,
    #[serde(default)]
    pub persist: bool,
    pub base_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortInspection {
    pub id: String,
    pub port: u16,
    pub in_use: bool,
    pub pid: Option<u32>,
    pub process_name: Option<String>,
    pub managed: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortsInspectOutput {
    pub items: Vec<PortInspection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortsSuggestInput {
    pub workspace_id: String,
    pub id: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortsSuggestOutput {
    pub candidates: Vec<u16>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortsAssignInput {
    pub workspace_id: String,
    pub id: String,
    pub port: u16,
    pub base_hash: String,
    #[serde(default)]
    pub restart: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortsAssignOutput {
    pub operation_id: Option<String>,
    pub spec: serde_yaml::Value,
    pub hash: String,
    pub restart_required: bool,
    /// §5.3：显式环境变量 / 自定义健康 URL 未跟随新端口的提示
    #[serde(default)]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretKeyStatus {
    pub key: String,
    pub source: String,
    pub present: bool,
    pub parse_ok: Option<bool>,
    pub git_tracked: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretsStatusOutput {
    pub backend: String,
    pub file: Option<String>,
    pub keys: Vec<SecretKeyStatus>,
    pub git_ignored: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretsSetInput {
    pub workspace_id: String,
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretsKeyOutput {
    pub ok: bool,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretsDeleteInput {
    pub workspace_id: String,
    pub key: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretsValidateInput {
    pub workspace_id: String,
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecretsValidateOutput {
    pub ok: bool,
    pub missing: Vec<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkSaveInput {
    pub workspace_id: Option<String>,
    pub config: serde_yaml::Value,
    pub base_hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct NetworkSaveOutput {
    pub ok: bool,
    pub hash: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogsSearchInput {
    pub workspace_id: String,
    pub source: Option<String>,
    pub query: String,
    #[serde(default)]
    pub case_sensitive: bool,
    pub limit: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LogsExportInput {
    pub workspace_id: String,
    pub source: Option<String>,
    pub query: Option<String>,
    pub format: String,
    pub destination_path: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricsSnapshotOutput {
    pub services: IndexMap<String, Option<ServiceMetrics>>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfileSummary {
    pub id: String,
    pub enabled_count: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilesListOutput {
    pub active: String,
    pub profiles: Vec<ProfileSummary>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilesActivateInput {
    pub workspace_id: String,
    pub id: String,
    pub base_hash: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ProfilesActivateOutput {
    pub spec: serde_yaml::Value,
    pub hash: String,
    pub active: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RuntimeBuildInput {
    pub workspace_id: String,
    pub id: String,
}

pub type ToolchainUpgradeInput = ToolchainInstallInput;

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn metrics_event_serializes_name() {
        let ev = MetricsEvent::new(
            "C:/work/mall",
            1,
            MetricsPayload {
                services: IndexMap::new(),
            },
        );
        let text = serde_yaml::to_string(&ev).unwrap();
        assert!(text.contains("st.metrics"));
        assert_eq!(ev.protocol, super::super::PROTOCOL);
    }
}
