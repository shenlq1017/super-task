use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum FeatureStatus {
    Live,
    Preview,
    Soon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feature {
    pub id: &'static str,
    pub path: &'static str,
    pub status: FeatureStatus,
    pub since: &'static str,
}

/// Backend + UI share this list. Soon features still appear in session.hello.
pub fn features() -> &'static [Feature] {
    &[
        Feature { id: "run", path: "/run", status: FeatureStatus::Live, since: "1.0" },
        Feature { id: "logs", path: "/logs", status: FeatureStatus::Live, since: "1.0" },
        Feature { id: "config", path: "/config", status: FeatureStatus::Live, since: "1.0" },
        Feature { id: "templates", path: "/templates", status: FeatureStatus::Live, since: "1.1" },
        Feature { id: "env", path: "/env", status: FeatureStatus::Live, since: "1.0" },
        Feature { id: "workspaces", path: "/workspaces", status: FeatureStatus::Live, since: "1.1" },
        Feature { id: "discover", path: "/discover", status: FeatureStatus::Live, since: "1.1" },
        Feature { id: "git", path: "/git", status: FeatureStatus::Live, since: "1.1" },
        Feature { id: "docker", path: "/docker", status: FeatureStatus::Live, since: "1.3" },
        Feature { id: "gateway", path: "/gateway", status: FeatureStatus::Soon, since: "1.6" },
        Feature { id: "cloud", path: "/cloud", status: FeatureStatus::Soon, since: "2.0" },
        Feature { id: "ai", path: "/ai", status: FeatureStatus::Soon, since: "2.1" },
        Feature { id: "settings", path: "/settings", status: FeatureStatus::Live, since: "1.0" },
    ]
}

pub fn require_live(id: &str) -> crate::error::Result<&'static Feature> {
    let Some(f) = features().iter().find(|f| f.id == id) else {
        return Err(crate::Error::new(
            crate::ErrorCode::NotFound,
            format!("未知功能 {id}"),
        ));
    };
    match f.status {
        FeatureStatus::Soon => Err(crate::Error::soon(f.since, id)),
        FeatureStatus::Preview | FeatureStatus::Live => Ok(f),
    }
}

pub const SOON_COMMANDS: &[(&str, &str)] = &[
    ("gateway.apply", "1.6"),
    ("cloud.login", "2.0"),
    ("cloud.sync", "2.0"),
    ("ai.complete", "2.1"),
];

pub fn reject_soon_command(cmd: &str) -> Option<crate::Error> {
    SOON_COMMANDS
        .iter()
        .find(|(c, _)| *c == cmd)
        .map(|(c, since)| crate::Error::soon(since, c))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn templates_is_live_since_1_1() {
        let f = require_live("templates").unwrap();
        assert_eq!(f.since, "1.1");
        let g = require_live("git").unwrap();
        assert_eq!(g.since, "1.1");
    }

    #[test]
    fn docker_is_live_since_1_3() {
        let f = require_live("docker").unwrap();
        assert_eq!(f.since, "1.3");
        assert!(reject_soon_command("docker.build").is_none());
    }
}
