//! 2.0 云模块（v2.0 规格）。本地优先：云是可选增强，未登录/离线零影响。
//!
//! - [`CloudProvider`] trait：登录/会话/实体 CRUD（`GitRunner` 式注入先例——
//!   生产 [`HttpCloudProvider`]，测试/CI [`FakeCloudProvider`]，**CI 零真实网络**）；
//! - [`sync`]：实体同步引擎（两阶段 pull→push + 冲突收集，rev 乐观并发）；
//! - [`crypto`]：密钥 vault 端到端加密（argon2id + XChaCha20-Poly1305）；
//! - [`migrate`]：一键迁移（实体落盘 + 工具链差量）；
//! - [`telemetry`]：opt-in 事件批量上报（默认关 = 零网络请求）。
//!
//! 协议真源：`docs/spec/cloud.md`。

pub mod crypto;
pub mod fake;
pub mod http;
pub mod migrate;
pub mod session;
pub mod sync;
pub mod telemetry;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::{Error, ErrorCode, Result};

/// 实体类型（spec §6.1）。未知 type 客户端 skip 并报告（前向兼容，v2.2 kind 实体依赖）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EntityType {
    #[serde(rename = "workspace")]
    Workspace,
    #[serde(rename = "template")]
    Template,
    #[serde(rename = "settings")]
    Settings,
    #[serde(rename = "secrets.vault")]
    SecretsVault,
}

impl EntityType {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Workspace => "workspace",
            Self::Template => "template",
            Self::Settings => "settings",
            Self::SecretsVault => "secrets.vault",
        }
    }
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "workspace" => Some(Self::Workspace),
            "template" => Some(Self::Template),
            "settings" => Some(Self::Settings),
            "secrets.vault" => Some(Self::SecretsVault),
            _ => None,
        }
    }
}

/// 实体数据载荷：明文 JSON（workspace/template/settings）或密文（vault）。
///
/// This is deliberately not `#[serde(untagged)]`: an untagged enum tries the
/// `Plain(Value)` branch first, which makes the encrypted `{blob, salt}`
/// envelope indistinguishable from ordinary JSON object data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EntityData {
    Plain(serde_json::Value),
    Encrypted { blob: String, salt: String },
}

impl Serialize for EntityData {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Plain(value) => value.serialize(serializer),
            Self::Encrypted { blob, salt } => {
                #[derive(Serialize)]
                struct Encrypted<'a> {
                    blob: &'a str,
                    salt: &'a str,
                }
                Encrypted { blob, salt }.serialize(serializer)
            }
        }
    }
}

impl<'de> Deserialize<'de> for EntityData {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = serde_json::Value::deserialize(deserializer)?;
        if let serde_json::Value::Object(ref object) = value {
            let blob = object.get("blob").and_then(serde_json::Value::as_str);
            let salt = object.get("salt").and_then(serde_json::Value::as_str);
            if let (Some(blob), Some(salt)) = (blob, salt) {
                return Ok(Self::Encrypted {
                    blob: blob.to_owned(),
                    salt: salt.to_owned(),
                });
            }
        }
        Ok(Self::Plain(value))
    }
}

/// 实体信封（spec §10）：`{id, type, rev, updated_at, updated_by, data}`。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: EntityType,
    pub rev: u64,
    #[serde(default)]
    pub updated_at: u64,
    #[serde(default)]
    pub updated_by: String,
    pub data: EntityData,
}

/// 登录响应：access（短时效）+ refresh（长时效）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LoginTokens {
    pub account_id: String,
    pub email: String,
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in_secs: u64,
}

/// provider 请求抽象：HTTP 语义的最小面（status + body），供错误映射与 fake 旋钮。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    pub status: u16,
    pub body: String,
}

/// 云 provider trait（spec §10 协议的最小客户端面）。
pub trait CloudProvider: Send + Sync {
    fn login(&self, email: &str, password: &str) -> Result<LoginTokens>;
    fn refresh(&self, refresh_token: &str) -> Result<LoginTokens>;
    fn list(&self, token: &str, entity_type: Option<EntityType>) -> Result<Vec<Entity>>;
    fn get(&self, token: &str, id: &str) -> Result<Entity>;
    /// PUT 带 base_rev 乐观并发；不匹配 → `CLOUD_SYNC_CONFLICT`。
    fn put(&self, token: &str, entity: &Entity, base_rev: u64) -> Result<Entity>;
    fn delete(&self, token: &str, id: &str) -> Result<()>;
    /// 遥测批量上报；provider 实现 no-op 语义由调用方保证（telemetry 模块）。
    fn telemetry_batch(&self, token: &str, events: &str) -> Result<()>;
    fn quota(&self, token: &str) -> Result<QuotaUsage>;
}

/// 配额用量（spec §10：按实体数 + 总字节数）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct QuotaUsage {
    pub entities: u64,
    pub entities_max: u64,
    pub bytes: u64,
    pub bytes_max: u64,
}

/// HTTP → CLOUD_* 错误码映射（spec §10 表）。
pub fn map_status(status: u16) -> ErrorCode {
    match status {
        401 | 403 => ErrorCode::CloudAuthFailed,
        409 => ErrorCode::CloudSyncConflict,
        413 | 429 => ErrorCode::CloudQuotaExceeded,
        _ => ErrorCode::CloudProtocolError,
    }
}

/// 解析 JSON 实体信封；无法解析 → `CLOUD_PROTOCOL_ERROR`。
pub fn parse_entity(body: &str) -> Result<Entity> {
    serde_json::from_str(body).map_err(|e| {
        Error::new(
            ErrorCode::CloudProtocolError,
            format!("实体信封解析失败: {e}"),
        )
    })
}

/// sha256 十六进制（设备 id / 内容 hash 公共小工具）。
pub fn sha256_hex(input: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(input);
    let out = h.finalize();
    out.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn auth_error(msg: impl Into<String>) -> Error {
    Error::new(ErrorCode::CloudAuthFailed, msg)
}

pub fn offline_error(msg: impl Into<String>) -> Error {
    Error::new(ErrorCode::CloudOffline, msg)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entity_roundtrip_and_unknown_type() {
        let e = Entity {
            id: "e1".into(),
            entity_type: EntityType::Workspace,
            rev: 3,
            updated_at: 42,
            updated_by: "dev".into(),
            data: EntityData::Plain(serde_json::json!({"name": "a"})),
        };
        let s = serde_json::to_string(&e).unwrap();
        let back: Entity = serde_json::from_str(&s).unwrap();
        assert_eq!(back, e);
        // 未知的 type 字符串 → None（调用方 skip + 报告）
        assert!(EntityType::parse("kind").is_none());
        assert!(EntityType::parse("secrets.vault").is_some());
    }

    #[test]
    fn status_mapping_table() {
        assert_eq!(map_status(401), ErrorCode::CloudAuthFailed);
        assert_eq!(map_status(403), ErrorCode::CloudAuthFailed);
        assert_eq!(map_status(409), ErrorCode::CloudSyncConflict);
        assert_eq!(map_status(413), ErrorCode::CloudQuotaExceeded);
        assert_eq!(map_status(429), ErrorCode::CloudQuotaExceeded);
        assert_eq!(map_status(500), ErrorCode::CloudProtocolError);
    }

    #[test]
    fn encrypted_entity_data_is_not_plain() {
        let value = serde_json::json!({
            "id": "vault",
            "type": "secrets.vault",
            "rev": 1,
            "data": { "blob": "ciphertext", "salt": "salt" }
        });
        let entity: Entity = serde_json::from_value(value).unwrap();
        assert_eq!(entity.entity_type, EntityType::SecretsVault);
        assert_eq!(
            entity.data,
            EntityData::Encrypted {
                blob: "ciphertext".into(),
                salt: "salt".into()
            }
        );
        let wire = serde_json::to_value(&entity.data).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({ "blob": "ciphertext", "salt": "salt" })
        );
    }

    #[test]
    fn plain_object_with_only_one_crypto_field_stays_plain() {
        let data: EntityData =
            serde_json::from_value(serde_json::json!({ "blob": "ordinary" })).unwrap();
        assert_eq!(
            data,
            EntityData::Plain(serde_json::json!({ "blob": "ordinary" }))
        );
    }
}
