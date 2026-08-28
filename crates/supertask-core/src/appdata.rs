//! 1.1/1.2 应用数据：`%APPDATA%/SuperTask/app.json` 的模型与原子读写。
//! Spec: `docs/spec/ipc.md` §10.5、`docs/plans/2026-08-27-v1-2-feature-spec.md` §12.2。
//!
//! core 不依赖 serde_json：app.json 用 serde_yaml 序列化（JSON 是 YAML 子集）。
//! app data 只存偏好与路径记录，不存代码、密钥、凭据、日志、指标历史。

use std::fs;
use std::path::{Path, PathBuf};

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};

/// 文件格式版本（1.2 起为 2）。
const VERSION: u32 = 2;
/// 最近工作区条数上限。
const RECENTS_CAP: usize = 20;

fn default_true() -> bool {
    true
}
fn default_auto() -> String {
    "auto".to_string()
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppNetwork {
    pub proxy_mode: String,
    pub http: Option<String>,
    pub https: Option<String>,
    pub no_proxy: Vec<String>,
}

impl Default for AppNetwork {
    fn default() -> Self {
        Self {
            proxy_mode: "off".to_string(),
            http: None,
            https: None,
            no_proxy: vec!["127.0.0.1".into(), "localhost".into(), "::1".into()],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppData {
    pub version: u32,
    pub recents: Vec<String>,
    pub last_workspace: Option<String>,
    pub theme: String,
    pub restore_last: bool,
    pub close_to_tray: bool,
    pub start_on_login: bool,
    pub update_check: bool,
    pub stale: Vec<String>,
    #[serde(default = "default_auto")]
    pub toolchain_manager: String,
    pub network: AppNetwork,
    #[serde(default = "default_true")]
    pub log_notifications: bool,
    #[serde(default = "default_true")]
    pub system_notifications: bool,
    #[serde(default = "default_true")]
    pub metrics_enabled: bool,
    /// Unknown keys preserved across v1 to v2 migrate and save.
    #[serde(default, flatten)]
    pub extra: IndexMap<String, Value>,
}

impl Default for AppData {
    fn default() -> Self {
        Self {
            version: VERSION,
            recents: Vec::new(),
            last_workspace: None,
            theme: "light".to_string(),
            restore_last: true,
            close_to_tray: true,
            start_on_login: false,
            update_check: true,
            stale: Vec::new(),
            toolchain_manager: default_auto(),
            network: AppNetwork::default(),
            log_notifications: true,
            system_notifications: true,
            metrics_enabled: true,
            extra: IndexMap::new(),
        }
    }
}

pub fn load_at(path: &Path) -> AppData {
    let mut data = match fs::read_to_string(path) {
        Ok(text) => serde_yaml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("app.json 解析失败，已回退默认值: {e}");
            AppData::default()
        }),
        Err(_) => return AppData::default(),
    };
    if data.version < VERSION {
        data.version = VERSION;
        if save_at(path, &data).is_err() {
            // in-memory upgraded value (new fields already defaulted); do not overwrite old file
        }
    }
    data
}

pub fn save_at(path: &Path, data: &AppData) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| Error::new(ErrorCode::Spawn, format!("创建应用数据目录失败: {e}")))?;
    }
    let text = serde_yaml::to_string(data)
        .map_err(|e| Error::new(ErrorCode::Spawn, format!("序列化应用数据失败: {e}")))?;
    let tmp = tmp_path(path);
    fs::write(&tmp, text)
        .map_err(|e| Error::new(ErrorCode::Spawn, format!("写入应用数据失败: {e}")))?;
    fs::rename(&tmp, path)
        .map_err(|e| Error::new(ErrorCode::Spawn, format!("替换应用数据失败: {e}")))?;
    Ok(())
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}

impl AppData {
    pub fn record_open(&mut self, path: &str) {
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_string());
        self.recents.truncate(RECENTS_CAP);
        self.last_workspace = Some(path.to_string());
    }

    pub fn mark_stale(&mut self, path: &str) {
        if !self.stale.iter().any(|p| p == path) {
            self.stale.push(path.to_string());
        }
    }

    pub fn merge_import(&mut self, recents: &[String], last: Option<&str>) {
        for path in recents {
            if !self.recents.contains(path) {
                self.recents.push(path.clone());
            }
        }
        self.recents.truncate(RECENTS_CAP);
        if self.last_workspace.is_none() {
            self.last_workspace = last.map(|s| s.to_string());
        }
    }

    pub fn is_fresh(&self) -> bool {
        self.recents.is_empty() && self.last_workspace.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_file(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("st-appdata-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.join("app.json")
    }

    #[test]
    fn serializes_camel_case_keys() {
        let mut data = AppData::default();
        data.last_workspace = Some(r"C:/work/mall".into());
        data.restore_last = false;
        let text = serde_yaml::to_string(&data).unwrap();
        assert!(text.contains("lastWorkspace"), "camelCase 键缺失:\n{text}");
        assert!(text.contains("restoreLast: false"));
        assert!(
            !text.contains("last_workspace"),
            "不允许 snake_case 泄漏:\n{text}"
        );
        assert!(text.contains("toolchainManager"));
        assert!(text.contains("logNotifications"));
        assert!(text.contains("systemNotifications"));
        assert!(text.contains("metricsEnabled"));
        assert!(text.contains("proxyMode"));
    }

    #[test]
    fn load_missing_returns_default() {
        let path = temp_file("missing");
        assert_eq!(load_at(&path), AppData::default());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn load_corrupt_returns_default() {
        let path = temp_file("corrupt");
        fs::write(&path, "{{{ not valid").unwrap();
        assert_eq!(load_at(&path), AppData::default());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn save_load_roundtrip() {
        let path = temp_file("roundtrip");
        let mut data = AppData::default();
        data.record_open(r"C:/work/a");
        data.record_open(r"C:/work/b");
        data.mark_stale(r"C:/work/gone");
        data.theme = "dark".into();
        data.start_on_login = true;
        save_at(&path, &data).unwrap();
        assert_eq!(load_at(&path), data);
        assert!(!tmp_path(&path).exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn save_over_leftover_tmp() {
        let path = temp_file("tmp-leftover");
        fs::write(tmp_path(&path), "垃圾残留").unwrap();
        save_at(&path, &AppData::default()).unwrap();
        assert_eq!(load_at(&path), AppData::default());
        assert!(!tmp_path(&path).exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn record_open_dedup_cap_and_last() {
        let mut data = AppData::default();
        for p in ["a", "b", "c", "a"] {
            data.record_open(p);
        }
        assert_eq!(data.recents, vec!["a", "c", "b"]);
        assert_eq!(data.last_workspace.as_deref(), Some("a"));
        for i in 0..30 {
            data.record_open(&format!("p{i}"));
        }
        assert_eq!(data.recents.len(), RECENTS_CAP);
        assert_eq!(data.recents[0], "p29");
        assert!(!data.recents.contains(&"a".to_string()));
    }

    #[test]
    fn merge_import_union_dedup_cap() {
        let mut data = AppData::default();
        data.record_open("srv-a");
        data.record_open("srv-b");
        let imported: Vec<String> = (0..25).map(|i| format!("imp{i}")).collect();
        data.merge_import(&imported, Some("imp0"));
        assert_eq!(data.recents.len(), RECENTS_CAP);
        assert_eq!(&data.recents[0], "srv-b");
        assert_eq!(&data.recents[1], "srv-a");
        assert_eq!(data.recents[2], "imp0");
        assert!(data.recents.last().unwrap().starts_with("imp"));
        assert_eq!(data.last_workspace.as_deref(), Some("srv-b"));

        let mut fresh = AppData::default();
        fresh.merge_import(&["x".to_string(), "y".to_string()], Some("y"));
        assert_eq!(fresh.recents, vec!["x", "y"]);
        assert_eq!(fresh.last_workspace.as_deref(), Some("y"));
    }

    #[test]
    fn mark_stale_dedup_and_keeps_recents() {
        let mut data = AppData::default();
        data.record_open("gone");
        data.mark_stale("gone");
        data.mark_stale("gone");
        assert_eq!(data.stale, vec!["gone"]);
        assert_eq!(data.recents, vec!["gone"]);
    }

    #[test]
    fn is_fresh_semantics() {
        assert!(AppData::default().is_fresh());
        let mut used = AppData::default();
        used.record_open("x");
        assert!(!used.is_fresh());
        let mut only_last = AppData::default();
        only_last.last_workspace = Some("x".into());
        assert!(!only_last.is_fresh());
    }

    #[test]
    fn v1_upgrades_to_v2_keeps_unknown_keys() {
        let path = temp_file("v1-upgrade");
        fs::write(
            &path,
            "version: 1\nrecents:\n- C:/work/a\ntheme: light\nrestoreLast: true\ncloseToTray: true\nstartOnLogin: false\nupdateCheck: true\nstale: []\ncustomFutureKey: keep-me\n",
        )
        .unwrap();
        let data = load_at(&path);
        assert_eq!(data.version, 2);
        assert_eq!(data.recents, vec!["C:/work/a".to_string()]);
        assert_eq!(data.toolchain_manager, "auto");
        assert_eq!(data.network.proxy_mode, "off");
        assert!(data.log_notifications);
        assert!(data.system_notifications);
        assert!(data.metrics_enabled);
        assert_eq!(
            data.extra.get("customFutureKey").and_then(|v| v.as_str()),
            Some("keep-me")
        );
        let disk = fs::read_to_string(&path).unwrap();
        assert!(disk.contains("customFutureKey"));
        let reloaded = load_at(&path);
        assert_eq!(reloaded.version, 2);
        assert_eq!(
            reloaded
                .extra
                .get("customFutureKey")
                .and_then(|v| v.as_str()),
            Some("keep-me")
        );
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn migrate_write_failure_keeps_old_file() {
        let path = temp_file("v1-migrate-fail");
        let v1 = "version: 1\nrecents:\n- C:/work/a\ncustomFutureKey: keep-me\n";
        fs::write(&path, v1).unwrap();
        let tmp = tmp_path(&path);
        fs::create_dir_all(&tmp).unwrap();
        let data = load_at(&path);
        assert_eq!(data.version, 2);
        assert_eq!(data.recents, vec!["C:/work/a".to_string()]);
        let disk = fs::read_to_string(&path).unwrap();
        assert!(disk.contains("version: 1"), "old file must stay: {disk}");
        assert!(disk.contains("customFutureKey"));
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }
}
