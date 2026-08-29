//! 同步引擎（v2.0 规格 §6）。两阶段 pull → push + 冲突收集：
//! - 服务端 per-entity 单调 rev；本地 state.json 记 `base_rev + last_synced_hash`；
//! - dirty = 当前内容 hash ≠ last_synced_hash；
//! - 冲突（本地 dirty 且服务端 rev 前进 / PUT 409）→ **两端内容都保留**，记入冲突列表；
//! - 解决（`resolve`）：keep-local / keep-server / keep-both（both 生成「副本」实体）；
//! - 打开中的工作区（锁定）：`write_local` 返回 `TargetLocked` → 挂起该实体（pending），
//!   绝不在服务运行中写 yaml；同步只落盘，绝不自动启动服务。

use std::collections::BTreeMap;

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use super::{sha256_hex, CloudProvider, Entity, EntityData, EntityType};
use crate::error::{Error, ErrorCode, Result};

/// 单个实体的本地同步状态（state.json 一行）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TrackedEntity {
    #[serde(rename = "type")]
    pub entity_type: EntityType,
    pub base_rev: u64,
    pub last_synced_hash: String,
    /// workspace：落盘根目录（拉取时用户选择后写入）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_path: Option<String>,
}

/// 本地同步状态（`%APPDATA%/SuperTask/cloud/state.json`）。
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SyncState {
    #[serde(default)]
    pub entities: IndexMap<String, TrackedEntity>,
    /// 最近一次成功同步时间（/cloud 展示）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_synced_ms: Option<u64>,
    /// 待解决冲突：entity_id → {local, server} 两端内容都保留。
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub conflicts: IndexMap<String, ConflictPair>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConflictPair {
    pub entity_type: EntityType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server: Option<serde_json::Value>,
    pub server_rev: u64,
}

pub fn content_hash(data: &serde_json::Value) -> String {
    // BTreeMap 化保证键序稳定（serde_json Value 对象键序不保证）
    let canonical = to_canonical(data).unwrap_or_else(|| data.to_string());
    sha256_hex(canonical.as_bytes())
}

fn to_canonical(v: &serde_json::Value) -> Option<String> {
    match v {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> = map
                .iter()
                .map(|(k, val)| (k.clone(), from_canonical(val)))
                .collect();
            serde_json::to_string(&sorted).ok()
        }
        other => Some(other.to_string()),
    }
}

fn from_canonical(v: &serde_json::Value) -> serde_json::Value {
    match v {
        serde_json::Value::Object(map) => {
            let sorted: BTreeMap<String, serde_json::Value> = map
                .iter()
                .map(|(k, val)| (k.clone(), from_canonical(val)))
                .collect();
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        other => other.clone(),
    }
}

/// 本地存储抽象：一个 entity type 的读写端（workspace / template / settings 各一实现）。
pub trait LocalBinding: Send + Sync {
    fn entity_type(&self) -> EntityType;
    /// 本地已有的实体 id 集合（用于「新远端实体」归属判断）。
    fn ids(&self) -> Vec<String>;
    /// 读取本地当前内容（None = 本地无此实体内容）。
    fn read(&self, id: &str) -> Option<serde_json::Value>;
    /// 拉取落盘。返回 false = 挂起（目标已有未同步 yaml / 工作区被锁 / 需要目标目录）。
    fn write(&mut self, id: &str, data: &serde_json::Value, state: &mut SyncState) -> Result<bool>;
    /// 冲突 keep-both 的本地副本命名（workspace：`<name> (copy N)`）。
    fn copy_id(&self, id: &str) -> String {
        format!("{id}-copy")
    }
}

#[derive(Debug, Default, PartialEq)]
pub struct SyncOutcome {
    pub pushed: usize,
    pub pulled: usize,
    /// 挂起（锁定 / 需选目标目录）。
    pub pending: Vec<(String, String)>,
    /// 未知实体类型（前向兼容：skip 并报告）。
    pub skipped: Vec<String>,
}

pub fn load_state() -> SyncState {
    let path = super::session::session_dir().join("state.json");
    std::fs::read_to_string(path)
        .ok()
        .and_then(|txt| serde_json::from_str(&txt).ok())
        .unwrap_or_default()
}

pub fn save_state(state: &SyncState) -> Result<()> {
    let dir = super::session::session_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("无法创建 cloud 目录: {e}")))?;
    let text = serde_json::to_string(state)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("同步状态序列化失败: {e}")))?;
    std::fs::write(dir.join("state.json"), text)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("同步状态写入失败: {e}")))
}

/// `cloud.sync` 主算法（spec §6.2）。
pub fn sync(
    provider: &dyn CloudProvider,
    token: &str,
    stores: &mut [&mut dyn LocalBinding],
    state: &mut SyncState,
    device: &str,
) -> Result<SyncOutcome> {
    let mut outcome = SyncOutcome::default();
    let remote = provider.list(token, None)?;

    // ---- pull：服务端 rev > 本地 base_rev（或新实体）→ 应用 ----
    for entity in &remote {
        match entity.data {
            EntityData::Plain(_) => {}
            EntityData::Encrypted { .. } => {
                // vault 实体不自动应用：拉取端需 passphrase 解包（命令层处理）
                continue;
            }
        }
        let tracking = state.entities.get(&entity.id);
        if tracking.is_some() && tracking.unwrap().base_rev >= entity.rev {
            continue; // 本地已是最新
        }
        let data_value = match &entity.data {
            EntityData::Plain(v) => v.clone(),
            EntityData::Encrypted { .. } => unreachable!(),
        };
        // 本地 dirty 且远端也前进 → 冲突，不覆盖
        let binding = stores
            .iter_mut()
            .find(|b| b.entity_type() == entity.entity_type);
        let Some(binding) = binding else {
            outcome
                .skipped
                .push(format!("{} (no binding)", entity.entity_type.as_str()));
            continue;
        };
        let local_content = binding.read(&entity.id);
        let synced_hash = tracking.map(|t| t.last_synced_hash.clone());
        let dirty = match (&local_content, &synced_hash) {
            (Some(content), Some(h)) => content_hash(content) != *h,
            (Some(_), None) => true, // 本地有内容但从未同步
            (None, _) => false,
        };
        if dirty {
            state.conflicts.insert(
                entity.id.clone(),
                ConflictPair {
                    entity_type: entity.entity_type,
                    local: local_content,
                    server: Some(data_value),
                    server_rev: entity.rev,
                },
            );
            continue;
        }
        let applied = binding.write(&entity.id, &data_value, state)?;
        if applied {
            state.entities.insert(
                entity.id.clone(),
                TrackedEntity {
                    entity_type: entity.entity_type,
                    base_rev: entity.rev,
                    last_synced_hash: content_hash(&data_value),
                    local_path: state
                        .entities
                        .get(&entity.id)
                        .and_then(|t| t.local_path.clone()),
                },
            );
            outcome.pulled += 1;
        } else {
            outcome
                .pending
                .push((entity.id.clone(), "目标已有未同步内容或工作区被锁定".into()));
        }
    }

    // ---- push：本地 dirty → PUT（base_rev 乐观并发）----
    for binding in stores.iter_mut() {
        let ids = binding.ids();
        for id in ids {
            let Some(content) = binding.read(&id) else {
                continue;
            };
            let tracking = state
                .entities
                .get(&id)
                .filter(|t| t.entity_type == binding.entity_type());
            let (base_rev, synced_hash) = match tracking {
                Some(t) => (t.base_rev, Some(t.last_synced_hash.clone())),
                None => (0, None),
            };
            let dirty = match &synced_hash {
                Some(h) => content_hash(&content) != *h,
                None => true,
            };
            if !dirty {
                continue;
            }
            // 有未解决冲突的实体暂不 push（先 resolve）
            if state.conflicts.contains_key(&id) {
                continue;
            }
            let content_for_push = content.clone();
            let entity = Entity {
                id: id.clone(),
                entity_type: binding.entity_type(),
                rev: if base_rev == 0 { 0 } else { base_rev + 1 },
                updated_at: now_ms(),
                updated_by: device.to_string(),
                data: EntityData::Plain(content_for_push),
            };
            match provider.put(token, &entity, base_rev) {
                Ok(put) => {
                    state.entities.insert(
                        id.clone(),
                        TrackedEntity {
                            entity_type: binding.entity_type(),
                            base_rev: put.rev,
                            last_synced_hash: content_hash(&content),
                            local_path: state.entities.get(&id).and_then(|t| t.local_path.clone()),
                        },
                    );
                    outcome.pushed += 1;
                }
                Err(e) if e.code() == ErrorCode::CloudSyncConflict => {
                    let server_rev = provider.get(token, &id).map(|s| s.rev).unwrap_or(0);
                    let server_data = provider.get(token, &id).ok().map(|s| match s.data {
                        EntityData::Plain(v) => v,
                        EntityData::Encrypted { blob, salt } => {
                            serde_json::json!({"blob": blob, "salt": salt})
                        }
                    });
                    state.conflicts.insert(
                        id.clone(),
                        ConflictPair {
                            entity_type: binding.entity_type(),
                            local: Some(content),
                            server: server_data,
                            server_rev,
                        },
                    );
                }
                Err(e) => return Err(e),
            }
        }
    }
    Ok(outcome)
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// `cloud.resolve`：冲突三选一（两端内容都保留，绝不静默覆盖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolveChoice {
    Local,
    Server,
    Both,
}

pub fn resolve(
    provider: &dyn CloudProvider,
    token: &str,
    stores: &mut [&mut dyn LocalBinding],
    state: &mut SyncState,
    entity_id: &str,
    choice: ResolveChoice,
    device: &str,
) -> Result<()> {
    let pair = state.conflicts.shift_remove(entity_id).ok_or_else(|| {
        Error::new(
            ErrorCode::NotFound,
            format!("实体 {entity_id} 无待解决冲突"),
        )
    })?;
    let binding = stores
        .iter_mut()
        .find(|b| b.entity_type() == pair.entity_type)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, "无对应本地存储"))?;
    match choice {
        ResolveChoice::Local => {
            // 强制推送本地内容（base = 服务端当前 rev）
            let local = pair
                .local
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "本地内容缺失"))?;
            let entity = Entity {
                id: entity_id.to_string(),
                entity_type: pair.entity_type,
                rev: pair.server_rev + 1,
                updated_at: now_ms(),
                updated_by: device.to_string(),
                data: EntityData::Plain(local.clone()),
            };
            let put = provider.put(token, &entity, pair.server_rev)?;
            state.entities.insert(
                entity_id.to_string(),
                TrackedEntity {
                    entity_type: pair.entity_type,
                    base_rev: put.rev,
                    last_synced_hash: content_hash(&local),
                    local_path: state
                        .entities
                        .get(entity_id)
                        .and_then(|t| t.local_path.clone()),
                },
            );
        }
        ResolveChoice::Server => {
            let server = pair
                .server
                .clone()
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "服务端内容缺失"))?;
            if binding.write(entity_id, &server, state)? {
                state.entities.insert(
                    entity_id.to_string(),
                    TrackedEntity {
                        entity_type: pair.entity_type,
                        base_rev: pair.server_rev,
                        last_synced_hash: content_hash(&server),
                        local_path: state
                            .entities
                            .get(entity_id)
                            .and_then(|t| t.local_path.clone()),
                    },
                );
            }
        }
        ResolveChoice::Both => {
            // 本地内容作为新实体（副本）推送；服务端版本应用到本地
            let local = pair
                .local
                .ok_or_else(|| Error::new(ErrorCode::NotFound, "本地内容缺失"))?;
            let copy_id = binding.copy_id(entity_id);
            let copy = Entity {
                id: copy_id.clone(),
                entity_type: pair.entity_type,
                rev: 0,
                updated_at: now_ms(),
                updated_by: device.to_string(),
                data: EntityData::Plain(local.clone()),
            };
            let put = provider.put(token, &copy, 0)?;
            state.entities.insert(
                copy_id,
                TrackedEntity {
                    entity_type: pair.entity_type,
                    base_rev: put.rev,
                    last_synced_hash: content_hash(&local),
                    local_path: None,
                },
            );
            if let Some(server) = &pair.server {
                if binding.write(entity_id, server, state)? {
                    state.entities.insert(
                        entity_id.to_string(),
                        TrackedEntity {
                            entity_type: pair.entity_type,
                            base_rev: pair.server_rev,
                            last_synced_hash: content_hash(server),
                            local_path: state
                                .entities
                                .get(entity_id)
                                .and_then(|t| t.local_path.clone()),
                        },
                    );
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cloud::fake::{FakeCloudProvider, FakeKnob};
    use std::collections::BTreeMap;
    use std::sync::RwLock;

    /// 内存 LocalBinding：workspace 语义（write 拒绝 = 工作区被锁/目标占用 → 挂起）。
    struct MemStore {
        kind: EntityType,
        data: RwLock<BTreeMap<String, serde_json::Value>>,
        reject_write: bool,
    }
    impl MemStore {
        fn new(kind: EntityType) -> Self {
            Self {
                kind,
                data: RwLock::new(BTreeMap::new()),
                reject_write: false,
            }
        }
        fn put(&self, id: &str, v: serde_json::Value) {
            self.data.write().unwrap().insert(id.to_string(), v);
        }
    }
    impl LocalBinding for MemStore {
        fn entity_type(&self) -> EntityType {
            self.kind
        }
        fn ids(&self) -> Vec<String> {
            self.data.read().unwrap().keys().cloned().collect()
        }
        fn read(&self, id: &str) -> Option<serde_json::Value> {
            self.data.read().unwrap().get(id).cloned()
        }
        fn write(
            &mut self,
            id: &str,
            data: &serde_json::Value,
            _state: &mut SyncState,
        ) -> Result<bool> {
            if self.reject_write {
                return Ok(false);
            }
            self.data
                .write()
                .unwrap()
                .insert(id.to_string(), data.clone());
            Ok(true)
        }
        fn copy_id(&self, id: &str) -> String {
            format!("{id}-copy")
        }
    }

    fn ws_entity(id: &str, name: &str, rev: u64) -> Entity {
        Entity {
            id: id.into(),
            entity_type: EntityType::Workspace,
            rev,
            updated_at: 0,
            updated_by: "other-device".into(),
            data: EntityData::Plain(serde_json::json!({ "name": name, "yaml": "version: 1" })),
        }
    }

    fn setup() -> (FakeCloudProvider, SyncState, MemStore) {
        (
            FakeCloudProvider::new(),
            SyncState::default(),
            MemStore::new(EntityType::Workspace),
        )
    }

    /// scoped 借用：sync/resolve 结束即释放 &mut store。
    fn run_sync(
        provider: &FakeCloudProvider,
        t: &str,
        stores: &mut [&mut dyn LocalBinding],
        state: &mut SyncState,
        dev: &str,
    ) -> Result<SyncOutcome> {
        sync(provider, t, stores, state, dev)
    }

    #[test]
    fn push_new_and_dirty_entities() {
        let (provider, mut state, mut store) = setup();
        store.put("w1", serde_json::json!({"name": "a", "yaml": "x"}));
        let out = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out.pushed, 1);
        let remote = provider.get("t", "w1").unwrap();
        assert_eq!(remote.rev, 1);
        // 干净 → 不再 push
        let out2 = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out2.pushed, 0);
        // dirty → push，rev+1
        store.put("w1", serde_json::json!({"name": "a", "yaml": "y"}));
        let out3 = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out3.pushed, 1);
        assert_eq!(provider.get("t", "w1").unwrap().rev, 2);
    }

    #[test]
    fn pull_applies_remote_without_overwriting_dirty_local() {
        let (provider, mut state, mut store) = setup();
        provider.seed(ws_entity("w1", "remote", 3));
        let out = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out.pulled, 1);
        assert_eq!(store.read("w1").unwrap()["name"], "remote");
        // 远端前进且本地 dirty → 冲突：两端保留、本地不被覆盖
        provider.seed(ws_entity("w1", "remote2", 4));
        store.put("w1", serde_json::json!({"name": "local-edit", "yaml": "z"}));
        let out2 = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out2.pulled, 0);
        assert!(state.conflicts.contains_key("w1"));
        let pair = state.conflicts.get("w1").unwrap();
        assert_eq!(pair.local.as_ref().unwrap()["name"], "local-edit");
        assert_eq!(pair.server.as_ref().unwrap()["name"], "remote2");
        assert_eq!(store.read("w1").unwrap()["name"], "local-edit");
    }

    #[test]
    fn resolve_three_choices() {
        let (provider, mut state, mut store) = setup();
        provider.seed(ws_entity("w1", "server-ver", 5));
        store.put("w1", serde_json::json!({"name": "local-ver", "yaml": "l"}));
        {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap();
        }
        assert!(state.conflicts.contains_key("w1"));

        resolve(
            &provider,
            "t",
            &mut [&mut store],
            &mut state,
            "w1",
            ResolveChoice::Local,
            "dev1",
        )
        .unwrap();
        assert_eq!(
            provider.get("t", "w1").unwrap().data,
            EntityData::Plain(serde_json::json!({"name": "local-ver", "yaml": "l"}))
        );
        assert!(!state.conflicts.contains_key("w1"));

        provider.seed(ws_entity("w1", "server-ver2", 9));
        store.put("w1", serde_json::json!({"name": "again", "yaml": "m"}));
        {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap();
        }
        resolve(
            &provider,
            "t",
            &mut [&mut store],
            &mut state,
            "w1",
            ResolveChoice::Server,
            "dev1",
        )
        .unwrap();
        assert_eq!(store.read("w1").unwrap()["name"], "server-ver2");
        assert!(!state.conflicts.contains_key("w1"));

        provider.seed(ws_entity("w1", "server-ver3", 12));
        store.put("w1", serde_json::json!({"name": "third", "yaml": "n"}));
        {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap();
        }
        resolve(
            &provider,
            "t",
            &mut [&mut store],
            &mut state,
            "w1",
            ResolveChoice::Both,
            "dev1",
        )
        .unwrap();
        let copy = provider.get("t", "w1-copy").unwrap();
        assert_eq!(
            copy.data,
            EntityData::Plain(serde_json::json!({"name": "third", "yaml": "n"}))
        );
        assert_eq!(store.read("w1").unwrap()["name"], "server-ver3");
    }

    #[test]
    fn offline_fails_command_local_state_untouched() {
        let (provider, mut state, mut store) = setup();
        provider.set_knob(FakeKnob::Offline);
        let e = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap_err()
        };
        assert_eq!(e.code(), ErrorCode::CloudOffline);
    }

    #[test]
    fn locked_workspace_pends_instead_of_failing() {
        let (provider, mut state, mut store) = setup();
        store.reject_write = true; // 工作区被引擎持有
        provider.seed(ws_entity("w1", "remote", 2));
        let out = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out.pulled, 0);
        assert_eq!(out.pending.len(), 1);
        // 关闭工作区后重试成功
        store.reject_write = false;
        let out2 = {
            let mut stores: [&mut dyn LocalBinding; 1] = [&mut store];
            run_sync(&provider, "t", &mut stores, &mut state, "dev1").unwrap()
        };
        assert_eq!(out2.pulled, 1);
    }
}
