//! 1.3 IPC data types（docker）。Command handlers land in later phases. Protocol stays 1.

use serde::{Deserialize, Serialize};

/// §9 ContainerSummary：`docker.ps` 单个容器（限于当前 compose project）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContainerSummary {
    pub service: String,
    pub container_id: String,
    pub image: String,
    /// running / exited / created / paused …（小写）
    pub state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub health: Option<String>,
    #[serde(default)]
    pub ports: Vec<u16>,
}

/// §9 ImageSummary：`docker.images` 单个镜像（本机只读列表）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImageSummary {
    pub repository: String,
    pub tag: String,
    pub id: String,
    pub size_bytes: Option<u64>,
    pub created_ms: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerPsOutput {
    pub containers: Vec<ContainerSummary>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerImagesOutput {
    pub images: Vec<ImageSummary>,
}

/// `docker.build` 输入：name 必须是 YAML `docker.builds` 中已定义条目（§6.2）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DockerBuildInput {
    pub workspace_id: String,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payload_field_names_are_snake_case() {
        let c = ContainerSummary {
            service: "redis".into(),
            container_id: "abc".into(),
            image: "redis:7".into(),
            state: "running".into(),
            health: Some("healthy".into()),
            ports: vec![6379],
        };
        let text = serde_yaml::to_string(&c).unwrap();
        assert!(text.contains("container_id"));
        assert!(text.contains("service: redis"));
        assert!(text.contains("health: healthy"));

        let i = ImageSummary {
            repository: "mall".into(),
            tag: "local".into(),
            id: "sha256:a".into(),
            size_bytes: Some(1),
            created_ms: None,
        };
        let text = serde_yaml::to_string(&i).unwrap();
        assert!(text.contains("size_bytes: 1"));
        assert!(text.contains("created_ms"));
    }
}
