use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;

#[derive(Debug)]
pub enum AppError {
    BadRequest(String),
    Unauthorized,
    NotFound,
    Conflict,
    Quota,
    Internal(String),
}

impl AppError {
    fn details(&self) -> (StatusCode, &'static str, String) {
        match self {
            Self::BadRequest(message) => (StatusCode::BAD_REQUEST, "BAD_REQUEST", message.clone()),
            Self::Unauthorized => (
                StatusCode::UNAUTHORIZED,
                "CLOUD_AUTH_FAILED",
                "认证失败".into(),
            ),
            Self::NotFound => (StatusCode::NOT_FOUND, "NOT_FOUND", "资源不存在".into()),
            Self::Conflict => (
                StatusCode::CONFLICT,
                "CLOUD_SYNC_CONFLICT",
                "实体修订冲突".into(),
            ),
            Self::Quota => (
                StatusCode::TOO_MANY_REQUESTS,
                "CLOUD_QUOTA_EXCEEDED",
                "已超过云端配额".into(),
            ),
            Self::Internal(message) => {
                tracing::error!(error = %message, "cloud server internal error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "INTERNAL_ERROR",
                    "服务端内部错误".into(),
                )
            }
        }
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let (status, code, message) = self.details();
        (
            status,
            Json(json!({
                "error": message,
                "code": code,
                "message": message,
            })),
        )
            .into_response()
    }
}

impl std::fmt::Display for AppError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadRequest(message) | Self::Internal(message) => formatter.write_str(message),
            Self::Unauthorized => formatter.write_str("认证失败"),
            Self::NotFound => formatter.write_str("资源不存在"),
            Self::Conflict => formatter.write_str("实体修订冲突"),
            Self::Quota => formatter.write_str("已超过云端配额"),
        }
    }
}

impl std::error::Error for AppError {}

impl From<sqlx::Error> for AppError {
    fn from(value: sqlx::Error) -> Self {
        Self::Internal(value.to_string())
    }
}

impl From<sqlx::migrate::MigrateError> for AppError {
    fn from(value: sqlx::migrate::MigrateError) -> Self {
        Self::Internal(format!("database migration failed: {value}"))
    }
}
