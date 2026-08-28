//! 1.1 应用数据：`%APPDATA%/SuperTask/app.json` 的模型与原子读写。
//! Spec: `docs/spec/ipc.md` §10.5、`docs/plans/2026-08-27-v1-1-feature-spec.md` §10.1。
//!
//! core 不依赖 serde_json：app.json 用 serde_yaml 序列化（JSON 是 YAML 子集，
//! 读取端兼容任意合法 JSON 文件），camelCase 键由 `rename_all` 保证。
//! app data 只存偏好与路径记录，不存代码、密钥、凭据、日志或完整环境变量。

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

/// 文件格式版本。
const VERSION: u32 = 1;
/// 最近工作区条数上限。
const RECENTS_CAP: usize = 20;

/// app.json 数据模型；缺失字段由 `serde(default)` 兜底（兼容增量演进）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AppData {
    pub version: u32,
    /// 最近工作区，最近在前，最多 20 条。
    pub recents: Vec<String>,
    pub last_workspace: Option<String>,
    /// 主题，当前 "light"。
    pub theme: String,
    pub restore_last: bool,
    pub close_to_tray: bool,
    pub start_on_login: bool,
    pub update_check: bool,
    /// 已知失效路径：标记不删除，便于用户恢复或自行清理。
    pub stale: Vec<String>,
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
        }
    }
}

/// 读取 app.json；文件不存在或解析失败一律回退 Default（不 panic、不上抛）。
pub fn load_at(path: &Path) -> AppData {
    match fs::read_to_string(path) {
        Ok(text) => serde_yaml::from_str(&text).unwrap_or_else(|e| {
            eprintln!("app.json 解析失败，已回退默认值: {e}");
            AppData::default()
        }),
        Err(_) => AppData::default(),
    }
}

/// 原子写入：自动建父目录，先写 `<name>.tmp` 再 rename 替换，
/// 避免进程中断留下半份文件。
pub fn save_at(path: &Path, data: &AppData) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| {
            Error::new(ErrorCode::Spawn, format!("创建应用数据目录失败: {e}"))
        })?;
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

/// `app.json` → `app.json.tmp`。
fn tmp_path(path: &Path) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(".tmp");
    PathBuf::from(s)
}

impl AppData {
    /// 记录一次打开：去重提到最前、cap 20，并设为 last_workspace。
    pub fn record_open(&mut self, path: &str) {
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_string());
        self.recents.truncate(RECENTS_CAP);
        self.last_workspace = Some(path.to_string());
    }

    /// 标记失效路径；已在列表则忽略；不改 recents（记录保留，不立即删除）。
    pub fn mark_stale(&mut self, path: &str) {
        if !self.stale.iter().any(|p| p == path) {
            self.stale.push(path.to_string());
        }
    }

    /// localStorage 一次性迁移：并集去重（已有记录优先），cap 20；
    /// last_workspace 仅在本地为空时采用迁移值。
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

    /// 是否从未使用（recents 空 && last_workspace None），决定要不要做 localStorage 迁移。
    pub fn is_fresh(&self) -> bool {
        self.recents.is_empty() && self.last_workspace.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 临时目录按进程 id + 用例名隔离（与仓库现有测试一致），结束清理。
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
        assert!(!text.contains("last_workspace"), "不允许 snake_case 泄漏:\n{text}");
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
        // 原子替换后 tmp 不残留
        assert!(!tmp_path(&path).exists());
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn save_over_leftover_tmp() {
        // 模拟上次中断残留的 tmp：再次 save 走 tmp 覆盖 + rename，结果不受影响
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
        // 去重提到最前，保持最近顺序
        assert_eq!(data.recents, vec!["a", "c", "b"]);
        assert_eq!(data.last_workspace.as_deref(), Some("a"));
        // cap 20
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
        // 已有记录优先在前，随后按导入顺序去重追加，cap 20
        assert_eq!(data.recents.len(), RECENTS_CAP);
        assert_eq!(&data.recents[0], "srv-b");
        assert_eq!(&data.recents[1], "srv-a");
        assert_eq!(data.recents[2], "imp0");
        assert!(data.recents.last().unwrap().starts_with("imp"));
        // 本地 last_workspace 已存在时不覆盖
        assert_eq!(data.last_workspace.as_deref(), Some("srv-b"));

        // 本地为空时采用迁移值
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
}
