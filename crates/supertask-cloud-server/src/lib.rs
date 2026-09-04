//! Minimal self-hostable SuperTask cloud server.
//!
//! The library entry point is intentionally small so integration tests can exercise
//! the exact same router as the binary without binding a TCP port.

pub mod admin;
pub mod admin_http;
pub mod auth;
pub mod config;
pub mod entities;
pub mod error;
pub mod http;
pub mod quota;
pub mod state;
pub mod telemetry;

use std::path::{Component, Path};

use axum::{
    body::Body,
    extract::{Path as UrlPath, State},
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::{delete, get, post, put},
    Router,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use state::AppState;

/// Build the HTTP application using an already migrated state.
pub fn app(state: AppState) -> Router {
    client_api(state.clone())
        .merge(admin_api(state.clone()))
        .merge(admin_console(state))
}

fn client_api(state: AppState) -> Router {
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
        .route("/telemetry/policy", get(http::get_telemetry_policy))
        .route("/telemetry/batch", post(http::post_telemetry))
        .layer(CorsLayer::permissive())
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

/// Admin surface. Deliberately merged after the permissive CORS layer so it keeps a
/// same-origin-only policy: the console is served by this process and the desktop
/// client never calls `/admin`.
fn admin_api(state: AppState) -> Router {
    Router::new()
        .route("/admin/api/status", get(admin_http::status))
        .route("/admin/api/login", post(admin_http::login))
        .route("/admin/api/refresh", post(admin_http::refresh))
        .route("/admin/api/me", get(admin_http::me))
        .route(
            "/admin/api/accounts",
            get(admin_http::list_accounts).post(admin_http::create_account),
        )
        .route("/admin/api/accounts/{id}/role", put(admin_http::set_role))
        .route(
            "/admin/api/accounts/{id}/disabled",
            put(admin_http::set_disabled),
        )
        .route(
            "/admin/api/accounts/{id}/password",
            put(admin_http::set_password),
        )
        .route(
            "/admin/api/accounts/{id}",
            delete(admin_http::delete_account),
        )
        .layer(CorsLayer::new())
        .with_state(state)
}

const CONSOLE_MISSING: &str = r#"<!doctype html>
<html lang="zh-CN"><meta charset="utf-8"><title>SuperTask 控制台未构建</title>
<body style="font:15px/1.7 system-ui;margin:4rem auto;max-width:38rem;color:#222326">
<h1 style="font-size:1.25rem">管理控制台尚未构建</h1>
<p>服务端没有在本进程的控制台目录中找到 <code>index.html</code>。请在仓库根目录构建前端后重启：</p>
<pre style="background:#f3f4f5;padding:.75rem 1rem;border-radius:8px;overflow:auto">npm run build:console</pre>
<p>如控制台安装在别处，用 <code>SUPERTASK_CONSOLE_DIR</code> 指向其 <code>dist</code> 目录。</p>
</body></html>"#;

fn admin_console(state: AppState) -> Router {
    Router::new()
        .route("/admin", get(|| async { Redirect("/admin/") }))
        .route("/admin/", get(serve_console_index))
        .route("/admin/{*asset}", get(serve_console_asset))
        .with_state(state)
}

/// Not a 301: browsers cache permanent redirects, and this target changes as soon as
/// the console is installed at a different path.
struct Redirect(&'static str);

impl IntoResponse for Redirect {
    fn into_response(self) -> Response {
        (StatusCode::TEMPORARY_REDIRECT, [(header::LOCATION, self.0)]).into_response()
    }
}

async fn serve_console_index(State(state): State<AppState>) -> Response {
    serve_console_file(&state.config.console_dir, "index.html").await
}

async fn serve_console_asset(
    State(state): State<AppState>,
    UrlPath(asset): UrlPath<String>,
) -> Response {
    let name = if asset.is_empty() {
        "index.html"
    } else {
        &asset
    };
    serve_console_file(&state.config.console_dir, name).await
}

async fn serve_console_file(dir: &Path, relative: &str) -> Response {
    let Some(path) = safe_join(dir, relative) else {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    };
    if !path.is_file() {
        return if relative == "index.html" {
            Html(CONSOLE_MISSING).into_response()
        } else {
            (StatusCode::NOT_FOUND, "not found").into_response()
        };
    }
    if !stays_inside(dir, &path) {
        return (StatusCode::NOT_FOUND, "not found").into_response();
    }
    match tokio::fs::read(&path).await {
        Ok(bytes) => (
            [(header::CONTENT_TYPE, content_type(relative))],
            Body::from(bytes),
        )
            .into_response(),
        Err(_) => (StatusCode::NOT_FOUND, "not found").into_response(),
    }
}

/// Map a URL segment below `/admin/` onto the console directory. Only plain relative
/// components are accepted, so no traversal or absolute path can reach the disk.
fn safe_join(dir: &Path, relative: &str) -> Option<std::path::PathBuf> {
    let candidate = std::path::Path::new(relative);
    let is_safe = !relative.is_empty()
        && candidate.is_relative()
        && candidate
            .components()
            .all(|part| matches!(part, Component::Normal(_)));
    if !is_safe {
        return None;
    }
    Some(dir.join(candidate))
}

fn stays_inside(dir: &Path, path: &Path) -> bool {
    match (dir.canonicalize(), path.canonicalize()) {
        (Ok(root), Ok(file)) => file.starts_with(&root),
        _ => false,
    }
}

fn content_type(path: &str) -> &'static str {
    match path.rsplit('.').next().unwrap_or("") {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "ico" => "image/x-icon",
        "woff" => "font/woff",
        "woff2" => "font/woff2",
        "txt" => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}
