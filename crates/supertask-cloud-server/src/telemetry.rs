use axum::http::HeaderMap;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;

use crate::{auth, error::AppError};

const MAX_EVENTS: usize = 256;
const MAX_EVENT_BYTES: usize = 4096;
const MAX_BATCH_BYTES: usize = 256 * 1024;

#[derive(Debug, Deserialize)]
pub struct TelemetryRequest {
    pub events: Vec<TelemetryEvent>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum TelemetryEvent {
    AppStart,
    AppStop,
    FeatureOpen { feature_id: String },
    ServiceStart { kind: String },
}

pub async fn record(
    pool: &SqlitePool,
    headers: &HeaderMap,
    request: TelemetryRequest,
) -> Result<(), AppError> {
    let account = auth::account_from_bearer(
        pool,
        headers.get("authorization").and_then(|v| v.to_str().ok()),
    )
    .await?;
    if request.events.len() > MAX_EVENTS {
        return Err(AppError::BadRequest("遥测事件数量过多".into()));
    }
    let body =
        serde_json::to_vec(&request.events).map_err(|e| AppError::BadRequest(e.to_string()))?;
    if body.len() > MAX_BATCH_BYTES {
        return Err(AppError::BadRequest("遥测请求过大".into()));
    }
    for event in &request.events {
        let valid = match event {
            TelemetryEvent::FeatureOpen { feature_id } => {
                !feature_id.is_empty() && feature_id.len() <= MAX_EVENT_BYTES
            }
            TelemetryEvent::ServiceStart { kind } => {
                !kind.is_empty() && kind.len() <= MAX_EVENT_BYTES
            }
            TelemetryEvent::AppStart | TelemetryEvent::AppStop => true,
        };
        if !valid {
            return Err(AppError::BadRequest("遥测事件字段无效".into()));
        }
    }
    sqlx::query("INSERT INTO telemetry_batches(account_id,received_at,event_count) VALUES(?,?,?)")
        .bind(account)
        .bind(auth::now())
        .bind(request.events.len() as i64)
        .execute(pool)
        .await?;
    Ok(())
}
