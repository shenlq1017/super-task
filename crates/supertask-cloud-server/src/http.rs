use axum::{
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{self, LoginRequest, RefreshRequest},
    entities::{self, Entity, PutEntity},
    error::AppError,
    quota,
    state::AppState,
    telemetry,
};

pub async fn healthz() -> impl IntoResponse {
    Json(json!({ "status": "ok" }))
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Result<Json<LoginRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<auth::LoginResponse>, AppError> {
    let Json(request) = request.map_err(|_| AppError::BadRequest("请求 JSON 无效".into()))?;
    let device = device_id(&headers);
    Ok(Json(auth::login(&state.pool, request, &device).await?))
}

pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Result<Json<RefreshRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<auth::LoginResponse>, AppError> {
    let Json(request) = request.map_err(|_| AppError::BadRequest("请求 JSON 无效".into()))?;
    let device = device_id(&headers);
    Ok(Json(auth::refresh(&state.pool, request, &device).await?))
}

#[derive(Debug, Deserialize)]
pub struct EntityQuery {
    #[serde(rename = "type")]
    pub entity_type: Option<String>,
}

pub async fn list_entities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<EntityQuery>,
) -> Result<Json<Vec<Entity>>, AppError> {
    if let Some(kind) = query.entity_type.as_deref() {
        entities::validate_type(kind)?;
    }
    Ok(Json(
        entities::list(&state.pool, &headers, query.entity_type.as_deref()).await?,
    ))
}

pub async fn get_entity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<Json<Entity>, AppError> {
    if !entities::valid_id(&id) {
        return Err(AppError::BadRequest("实体 id 无效".into()));
    }
    Ok(Json(entities::get(&state.pool, &headers, &id).await?))
}

pub async fn put_entity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: Result<Json<PutEntity>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<Entity>, AppError> {
    let Json(request) = request.map_err(|_| AppError::BadRequest("请求 JSON 无效".into()))?;
    Ok(Json(
        entities::put(&state.pool, &headers, &id, request, &state.config).await?,
    ))
}

pub async fn delete_entity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    if !entities::valid_id(&id) {
        return Err(AppError::BadRequest("实体 id 无效".into()));
    }
    entities::delete(&state.pool, &headers, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn get_quota(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<quota::Quota>, AppError> {
    Ok(Json(
        quota::get(&state.pool, &headers, &state.config).await?,
    ))
}

pub async fn post_telemetry(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: Result<Json<telemetry::TelemetryRequest>, axum::extract::rejection::JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(request) = request.map_err(|_| AppError::BadRequest("请求 JSON 无效".into()))?;
    telemetry::record(&state.pool, &headers, request).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) fn device_id(headers: &HeaderMap) -> String {
    headers
        .get("x-device-id")
        .and_then(|value| value.to_str().ok())
        .filter(|value| !value.is_empty() && value.len() <= 128)
        .unwrap_or("server-device")
        .to_string()
}

#[allow(dead_code)]
fn bearer(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
}

#[allow(dead_code)]
fn _json_value(_: Value) {}
