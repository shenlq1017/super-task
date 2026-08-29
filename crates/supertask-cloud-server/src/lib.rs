//! Minimal self-hostable SuperTask cloud server.
//!
//! The library entry point is intentionally small so integration tests can exercise
//! the exact same router as the binary without binding a TCP port.

pub mod auth;
pub mod config;
pub mod entities;
pub mod error;
pub mod http;
pub mod quota;
pub mod state;
pub mod telemetry;

use axum::{
    routing::{get, post},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use state::AppState;

/// Build the HTTP application using an already migrated state.
pub fn app(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(http::healthz))
        .route("/auth/login", post(http::login))
        .route("/auth/refresh", post(http::refresh))
        .route("/entities", get(http::list_entities))
        .route(
            "/entities/{id}",
            get(http::get_entity)
                .put(http::put_entity)
                .delete(http::delete_entity),
        )
        .route("/quota", get(http::get_quota))
        .route("/telemetry/batch", post(http::post_telemetry))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
