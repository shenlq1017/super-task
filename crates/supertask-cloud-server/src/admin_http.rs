use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use serde::Deserialize;

use crate::{
    admin::{self},
    auth::{self, LoginRequest, RefreshRequest},
    entities,
    error::AppError,
    http::device_id,
    state::AppState,
};

type JsonBody<T> = Result<Json<T>, axum::extract::rejection::JsonRejection>;

fn invalid_json() -> AppError {
    AppError::BadRequest("请求 JSON 无效".into())
}

fn check_id(id: &str) -> Result<(), AppError> {
    if !entities::valid_id(id) {
        return Err(AppError::BadRequest("账号 id 无效".into()));
    }
    Ok(())
}

/// Unauthenticated setup probe: lets the console tell "no admin bootstrapped yet"
/// apart from a wrong password without exposing any account data.
pub async fn status(State(state): State<AppState>) -> Result<Json<admin::AdminStatus>, AppError> {
    Ok(Json(admin::status(&state.pool, &state.config).await?))
}

pub async fn login(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: JsonBody<LoginRequest>,
) -> Result<Json<auth::LoginResponse>, AppError> {
    let Json(request) = request.map_err(|_| invalid_json())?;
    Ok(Json(
        admin::admin_login(&state.pool, request, &device_id(&headers)).await?,
    ))
}

pub async fn refresh(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: JsonBody<RefreshRequest>,
) -> Result<Json<auth::LoginResponse>, AppError> {
    let Json(request) = request.map_err(|_| invalid_json())?;
    Ok(Json(
        admin::admin_refresh(&state.pool, request, &device_id(&headers)).await?,
    ))
}

pub async fn me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<admin::AdminMe>, AppError> {
    Ok(Json(admin::me(&state.pool, &headers).await?))
}

#[derive(Debug, Deserialize)]
pub struct AccountQuery {
    pub query: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

pub async fn list_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<AccountQuery>,
) -> Result<Json<Vec<admin::AccountRow>>, AppError> {
    admin::require_admin(&state.pool, &headers).await?;
    Ok(Json(
        admin::list(&state.pool, query.query.as_deref(), query.limit, query.offset).await?,
    ))
}

pub async fn create_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    request: JsonBody<admin::CreateAccount>,
) -> Result<impl IntoResponse, AppError> {
    let actor = admin::require_admin(&state.pool, &headers).await?;
    let Json(request) = request.map_err(|_| invalid_json())?;
    let row = admin::create(&state.pool, request).await?;
    tracing::info!(actor = %actor.account_id, action = "create", target = %row.id, "cloud admin mutation");
    Ok((StatusCode::CREATED, Json(row)))
}

pub async fn set_role(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: JsonBody<admin::SetRole>,
) -> Result<Json<admin::AccountRow>, AppError> {
    let actor = admin::require_admin(&state.pool, &headers).await?;
    check_id(&id)?;
    let Json(request) = request.map_err(|_| invalid_json())?;
    Ok(Json(admin::set_role(&state.pool, &actor, &id, request).await?))
}

pub async fn set_disabled(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: JsonBody<admin::SetDisabled>,
) -> Result<Json<admin::AccountRow>, AppError> {
    let actor = admin::require_admin(&state.pool, &headers).await?;
    check_id(&id)?;
    let Json(request) = request.map_err(|_| invalid_json())?;
    Ok(Json(
        admin::set_disabled(&state.pool, &actor, &id, request).await?,
    ))
}

pub async fn set_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    request: JsonBody<admin::SetPassword>,
) -> Result<StatusCode, AppError> {
    let actor = admin::require_admin(&state.pool, &headers).await?;
    check_id(&id)?;
    let Json(request) = request.map_err(|_| invalid_json())?;
    admin::set_password(&state.pool, &actor, &id, request).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn delete_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Result<StatusCode, AppError> {
    let actor = admin::require_admin(&state.pool, &headers).await?;
    check_id(&id)?;
    admin::delete(&state.pool, &actor, &id).await?;
    Ok(StatusCode::NO_CONTENT)
}
