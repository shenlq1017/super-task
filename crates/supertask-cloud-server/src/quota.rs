use axum::http::HeaderMap;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::{auth, config::Config, error::AppError};

#[derive(Debug, Clone, Serialize)]
pub struct Quota {
    pub entities: u64,
    pub entities_max: u64,
    pub bytes: u64,
    pub bytes_max: u64,
}

pub async fn get(
    pool: &SqlitePool,
    headers: &HeaderMap,
    config: &Config,
) -> Result<Quota, AppError> {
    let account = auth::account_from_bearer(
        pool,
        headers.get("authorization").and_then(|v| v.to_str().ok()),
    )
    .await?;
    let entities: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM entities WHERE account_id=?")
        .bind(account.clone())
        .fetch_one(pool)
        .await?;
    let bytes: i64 =
        sqlx::query_scalar("SELECT COALESCE(SUM(byte_size),0) FROM entities WHERE account_id=?")
            .bind(account)
            .fetch_one(pool)
            .await?;
    Ok(Quota {
        entities: entities.max(0) as u64,
        entities_max: config.entities_max,
        bytes: bytes.max(0) as u64,
        bytes_max: config.bytes_max,
    })
}
