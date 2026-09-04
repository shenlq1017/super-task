//! FakeCloudProvider：内存实现 + 故障旋钮（v2.0 实现计划 1.2）。
//! 测试与 CI 全走这里，**零真实网络**；每旋钮一测。

use std::sync::Mutex;

use super::{CloudProvider, Entity, EntityData, EntityType, LoginTokens, QuotaUsage};
use crate::error::{Error, ErrorCode, Result};

/// 可注入的故障/行为旋钮。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FakeKnob {
    /// 正常路径。
    #[default]
    None,
    /// 登录失败（AUTH_FAILED）。
    AuthFail,
    /// 列表/PUT 返回 409。
    Conflict,
    /// 网络不可达（OFFLINE）。
    Offline,
    /// PUT 配额超限（429）。
    Quota,
    /// 服务端 500（PROTOCOL_ERROR）。
    ServerError,
}

#[derive(Default)]
struct Inner {
    entities: Vec<Entity>,
    next_id: u64,
    knob: FakeKnob,
    /// 收到的遥测批次原文（断言「零请求」「无明文」用）。
    telemetry_requests: Vec<String>,
    /// 收到的 PUT 明文（场景 7 断言明文不出现在请求）。
    put_bodies: Vec<String>,
}

pub struct FakeCloudProvider {
    inner: Mutex<Inner>,
}

impl Default for FakeCloudProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl FakeCloudProvider {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(Inner {
                next_id: 100,
                ..Default::default()
            }),
        }
    }

    pub fn set_knob(&self, knob: FakeKnob) {
        self.inner.lock().unwrap().knob = knob;
    }

    /// 直接植入服务端实体（模拟另一台设备已推送）。
    pub fn seed(&self, entity: Entity) {
        self.inner.lock().unwrap().entities.push(entity);
    }

    pub fn entities(&self) -> Vec<Entity> {
        self.inner.lock().unwrap().entities.clone()
    }

    pub fn telemetry_requests(&self) -> Vec<String> {
        self.inner.lock().unwrap().telemetry_requests.clone()
    }

    pub fn put_bodies(&self) -> Vec<String> {
        self.inner.lock().unwrap().put_bodies.clone()
    }

    fn check(&self, deny: FakeKnob, code: ErrorCode, msg: &str) -> Result<()> {
        let knob = self.inner.lock().unwrap().knob;
        if knob == deny {
            return Err(Error::new(code, msg));
        }
        Ok(())
    }
}

impl CloudProvider for FakeCloudProvider {
    fn login(&self, email: &str, _password: &str) -> Result<LoginTokens> {
        self.check(FakeKnob::AuthFail, ErrorCode::CloudAuthFailed, "登录失败")?;
        self.check(FakeKnob::Offline, ErrorCode::CloudOffline, "网络不可达")?;
        Ok(LoginTokens {
            account_id: format!("acc-{email}"),
            email: email.to_string(),
            access_token: "fake-access".into(),
            refresh_token: "fake-refresh".into(),
            expires_in_secs: 900,
        })
    }

    fn refresh(&self, _refresh_token: &str) -> Result<LoginTokens> {
        self.check(
            FakeKnob::AuthFail,
            ErrorCode::CloudAuthFailed,
            "refresh 失效",
        )?;
        Ok(LoginTokens {
            account_id: "acc".into(),
            email: "a@b.c".into(),
            access_token: "fake-access-2".into(),
            refresh_token: "fake-refresh-2".into(),
            expires_in_secs: 900,
        })
    }

    fn list(&self, _token: &str, entity_type: Option<EntityType>) -> Result<Vec<Entity>> {
        self.check(FakeKnob::Offline, ErrorCode::CloudOffline, "网络不可达")?;
        self.check(
            FakeKnob::ServerError,
            ErrorCode::CloudProtocolError,
            "服务端异常",
        )?;
        Ok(self
            .inner
            .lock()
            .unwrap()
            .entities
            .iter()
            .filter(|e| entity_type.is_none_or(|t| e.entity_type == t))
            .cloned()
            .collect())
    }

    fn get(&self, _token: &str, id: &str) -> Result<Entity> {
        self.inner
            .lock()
            .unwrap()
            .entities
            .iter()
            .find(|e| e.id == id)
            .cloned()
            .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("实体不存在 {id}")))
    }

    fn put(&self, _token: &str, entity: &Entity, base_rev: u64) -> Result<Entity> {
        self.check(FakeKnob::Offline, ErrorCode::CloudOffline, "网络不可达")?;
        self.check(FakeKnob::Quota, ErrorCode::CloudQuotaExceeded, "超配额")?;
        self.check(
            FakeKnob::ServerError,
            ErrorCode::CloudProtocolError,
            "服务端异常",
        )?;
        let mut g = self.inner.lock().unwrap();
        g.put_bodies
            .push(serde_json::to_string(entity).unwrap_or_default());
        if let Some(e) = g.entities.iter_mut().find(|e| e.id == entity.id) {
            if e.rev != base_rev {
                let body = serde_json::json!({
                    "code": "CLOUD_SYNC_CONFLICT",
                    "message": "实体修订冲突",
                    "current": e.clone(),
                })
                .to_string();
                return Err(Error::new(ErrorCode::CloudSyncConflict, body));
            }
            e.rev += 1;
            e.updated_at = entity.updated_at;
            e.updated_by = entity.updated_by.clone();
            e.data = entity.data.clone();
            if entity.name.is_some() {
                e.name = entity.name.clone();
            }
            return Ok(e.clone());
        }
        // 新建
        let id = entity.id.clone();
        let created = Entity {
            id: id.clone(),
            entity_type: entity.entity_type,
            name: entity.name.clone(),
            rev: 1,
            updated_at: entity.updated_at,
            updated_by: entity.updated_by.clone(),
            data: entity.data.clone(),
        };
        let _ = id;
        g.next_id += 1;
        g.entities.push(created.clone());
        Ok(created)
    }

    fn delete(&self, _token: &str, id: &str) -> Result<()> {
        self.inner.lock().unwrap().entities.retain(|e| e.id != id);
        Ok(())
    }

    fn telemetry_batch(&self, _token: &str, events: &str) -> Result<()> {
        self.inner
            .lock()
            .unwrap()
            .telemetry_requests
            .push(events.to_string());
        Ok(())
    }

    fn quota(&self, _token: &str) -> Result<QuotaUsage> {
        let g = self.inner.lock().unwrap();
        Ok(QuotaUsage {
            entities: g.entities.len() as u64,
            entities_max: 100,
            bytes: g.entities.iter().map(|e| entity_size(e) as u64).sum(),
            bytes_max: 10_000_000,
            by_type: vec![],
        })
    }
}

fn entity_size(e: &Entity) -> usize {
    match &e.data {
        EntityData::Plain(v) => serde_json::to_string(v).map(|s| s.len()).unwrap_or(0),
        EntityData::Encrypted { blob, salt } => blob.len() + salt.len(),
    }
}
