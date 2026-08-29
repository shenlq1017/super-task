use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::{Row, SqlitePool};

use crate::{auth, error::AppError};

const MAX_ID_BYTES: usize = 128;
const MAX_TYPE_BYTES: usize = 64;

#[derive(Debug, Clone, Serialize)]
pub struct Entity {
    pub id: String,
    #[serde(rename = "type")]
    pub entity_type: String,
    pub rev: u64,
    pub updated_at: u64,
    pub updated_by: String,
    pub data: Value,
}

#[derive(Debug, Deserialize)]
pub struct PutEntity {
    #[serde(rename = "type")]
    pub entity_type: String,
    pub data: Value,
    pub base_rev: u64,
    #[serde(default)]
    pub updated_by: Option<String>,
}

pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID_BYTES
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_'))
}

pub fn validate_type(entity_type: &str) -> Result<(), AppError> {
    // Keep the server forward-compatible with future client entity kinds while
    // still rejecting whitespace, controls, and path-like delimiters.
    if entity_type.is_empty()
        || entity_type.len() > MAX_TYPE_BYTES
        || !entity_type
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
    {
        return Err(AppError::BadRequest("实体 type 无效".into()));
    }
    Ok(())
}

fn auth_header(headers: &HeaderMap) -> Option<&str> {
    headers.get("authorization").and_then(|v| v.to_str().ok())
}

pub async fn list(
    pool: &SqlitePool,
    headers: &HeaderMap,
    entity_type: Option<&str>,
) -> Result<Vec<Entity>, AppError> {
    let account = auth::account_from_bearer(pool, auth_header(headers)).await?;
    let rows = if let Some(kind) = entity_type {
        sqlx::query("SELECT id,type,rev,updated_at,updated_by,data FROM entities WHERE account_id=? AND type=? ORDER BY id")
            .bind(&account)
            .bind(kind)
            .fetch_all(pool)
            .await?
    } else {
        sqlx::query("SELECT id,type,rev,updated_at,updated_by,data FROM entities WHERE account_id=? ORDER BY id")
            .bind(&account)
            .fetch_all(pool)
            .await?
    };
    rows.into_iter().map(row_entity).collect()
}

pub async fn get(pool: &SqlitePool, headers: &HeaderMap, id: &str) -> Result<Entity, AppError> {
    let account = auth::account_from_bearer(pool, auth_header(headers)).await?;
    let row = sqlx::query(
        "SELECT id,type,rev,updated_at,updated_by,data FROM entities WHERE account_id=? AND id=?",
    )
    .bind(account)
    .bind(id)
    .fetch_optional(pool)
    .await?;
    row.map(row_entity).transpose()?.ok_or(AppError::NotFound)
}

pub async fn put(
    pool: &SqlitePool,
    headers: &HeaderMap,
    id: &str,
    req: PutEntity,
    config: &crate::config::Config,
) -> Result<Entity, AppError> {
    if !valid_id(id) {
        return Err(AppError::BadRequest("实体 id 无效".into()));
    }
    validate_type(&req.entity_type)?;
    if !req.data.is_object() {
        return Err(AppError::BadRequest("实体 data 必须是 JSON 对象".into()));
    }
    let account = auth::account_from_bearer(pool, auth_header(headers)).await?;
    let data_text =
        serde_json::to_string(&req.data).map_err(|e| AppError::Internal(e.to_string()))?;
    let bytes = data_text.len() as u64;
    if bytes > config.bytes_max {
        return Err(AppError::Quota);
    }

    let mut tx = pool.begin().await?;
    let row = sqlx::query("SELECT type,rev FROM entities WHERE account_id=? AND id=?")
        .bind(&account)
        .bind(id)
        .fetch_optional(&mut *tx)
        .await?;
    let current: Option<u64> = row
        .as_ref()
        .map(|r| r.try_get::<i64, _>("rev").map(|v| v as u64))
        .transpose()?;
    match current {
        Some(rev) if rev != req.base_rev => return Err(AppError::Conflict),
        None if req.base_rev != 0 => return Err(AppError::Conflict),
        _ => {}
    }
    if let Some(row) = row.as_ref() {
        let old_type: String = row.try_get("type")?;
        if old_type != req.entity_type {
            return Err(AppError::BadRequest("实体 type 不可修改".into()));
        }
    }
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE account_id=?")
        .bind(&account)
        .fetch_one(&mut *tx)
        .await?;
    let total: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(byte_size),0) FROM entities WHERE account_id=?")
            .bind(&account)
            .fetch_one(&mut *tx)
            .await?;
    let old_bytes: i64 =
        sqlx::query_scalar("SELECT byte_size FROM entities WHERE account_id=? AND id=?")
            .bind(&account)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await?
            .map(|v: i64| v)
            .unwrap_or(0);
    let new_count = count + i64::from(current.is_none());
    let new_total = total - old_bytes + bytes as i64;
    if new_count < 0
        || new_count as u64 > config.entities_max
        || new_total < 0
        || new_total as u64 > config.bytes_max
    {
        return Err(AppError::Quota);
    }
    let rev = current.map(|v| v + 1).unwrap_or(1);
    let updated_at = auth::now();
    let updated_by = req
        .updated_by
        .filter(|v| !v.is_empty() && v.len() <= 128)
        .unwrap_or_else(|| "server".into());
    sqlx::query("INSERT INTO entities(account_id,id,type,rev,updated_at,updated_by,data,byte_size) VALUES(?,?,?,?,?,?,?,?) ON CONFLICT(account_id,id) DO UPDATE SET type=excluded.type,rev=excluded.rev,updated_at=excluded.updated_at,updated_by=excluded.updated_by,data=excluded.data,byte_size=excluded.byte_size")
        .bind(&account)
        .bind(id)
        .bind(&req.entity_type)
        .bind(rev as i64)
        .bind(updated_at)
        .bind(&updated_by)
        .bind(&data_text)
        .bind(bytes as i64)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(Entity {
        id: id.into(),
        entity_type: req.entity_type,
        rev,
        updated_at: updated_at as u64,
        updated_by,
        data: req.data,
    })
}

pub async fn delete(pool: &SqlitePool, headers: &HeaderMap, id: &str) -> Result<(), AppError> {
    let account = auth::account_from_bearer(pool, auth_header(headers)).await?;
    sqlx::query("DELETE FROM entities WHERE account_id=? AND id=?")
        .bind(account)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

fn row_entity(row: sqlx::sqlite::SqliteRow) -> Result<Entity, AppError> {
    let text: String = row.try_get("data")?;
    Ok(Entity {
        id: row.try_get("id")?,
        entity_type: row.try_get("type")?,
        rev: row.try_get::<i64, _>("rev")? as u64,
        updated_at: row.try_get::<i64, _>("updated_at")? as u64,
        updated_by: row.try_get("updated_by")?,
        data: serde_json::from_str(&text).map_err(|e| AppError::Internal(e.to_string()))?,
    })
}
