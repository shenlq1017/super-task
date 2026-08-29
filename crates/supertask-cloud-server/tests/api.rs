use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use tower::util::ServiceExt;

use supertask_cloud_server::{app, auth, config::Config, state::AppState};

async fn test_app() -> axum::Router {
    let config = Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: ":memory:".into(),
        seed: false,
        seed_email: "test@example.invalid".into(),
        seed_password: None,
        entities_max: 10,
        bytes_max: 100_000,
    };
    let state = AppState::connect(config).await.unwrap();
    auth::seed_account(&state.pool, "test@example.invalid", "test-password")
        .await
        .unwrap();
    app(state)
}

async fn call(app: &mut axum::Router, request: Request<Body>) -> (StatusCode, Value) {
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 2 * 1024 * 1024)
        .await
        .unwrap();
    let value = if body.is_empty() {
        Value::Null
    } else {
        serde_json::from_slice(&body).unwrap()
    };
    (status, value)
}

async fn login(app: &mut axum::Router) -> String {
    let (status, body) = call(
        app,
        Request::post("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"test@example.invalid","password":"test-password"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    body["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn health_login_refresh_and_auth_errors() {
    let mut app = test_app().await;
    let (status, body) = call(
        &mut app,
        Request::get("/healthz").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["status"], "ok");

    let (status, body) = call(
        &mut app,
        Request::post("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"test@example.invalid","password":"wrong"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["error"], "认证失败");
    assert_eq!(body["code"], "CLOUD_AUTH_FAILED");

    let access = login(&mut app).await;
    let (status, body) = call(
        &mut app,
        Request::post("/auth/refresh")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"refresh_token":"not-a-token"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "CLOUD_AUTH_FAILED");
    let (status, _) = call(
        &mut app,
        Request::get("/quota").body(Body::empty()).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(!access.is_empty());
}

#[tokio::test]
async fn entity_crud_conflict_quota_and_telemetry() {
    let mut app = test_app().await;
    let token = login(&mut app).await;
    let auth = format!("Bearer {token}");
    let put = |base_rev: u64, data: Value| {
        Request::put("/entities/ws-1")
            .header("authorization", &auth)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"type":"workspace","data":data,"base_rev":base_rev,"updated_by":"device-1"})
                    .to_string(),
            ))
            .unwrap()
    };

    let (status, body) = call(&mut app, put(0, json!({"name":"one"}))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["rev"], 1);
    assert_eq!(body["type"], "workspace");

    let (status, body) = call(&mut app, put(0, json!({"name":"stale"}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "CLOUD_SYNC_CONFLICT");

    let (status, body) = call(
        &mut app,
        Request::get("/entities?type=workspace")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    let (status, body) = call(
        &mut app,
        Request::get("/entities/ws-1")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["data"]["name"], "one");

    let (status, body) = call(
        &mut app,
        Request::get("/quota")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["entities"], 1);
    assert_eq!(body["bytes_max"], 100000);

    let (status, _) = call(&mut app, Request::post("/telemetry/batch").header("authorization", &auth).header("content-type", "application/json").body(Body::from(json!({"events":[{"event":"app_start"},{"event":"feature_open","feature_id":"run"}]}).to_string())).unwrap()).await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    let (status, _) = call(
        &mut app,
        Request::delete("/entities/ws-1")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, body) = call(
        &mut app,
        Request::get("/entities/ws-1")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["error"], "资源不存在");
}

#[tokio::test]
async fn unknown_entity_type_is_stored_and_filtered() {
    let mut app = test_app().await;
    let token = login(&mut app).await;
    let auth = format!("Bearer {token}");
    let response = call(
        &mut app,
        Request::put("/entities/future-1")
            .header("authorization", &auth)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({
                    "type": "kind.python",
                    "data": {"entry": "main.py"},
                    "base_rev": 0,
                    "updated_by": "device-1"
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(response.0, StatusCode::OK);
    assert_eq!(response.1["type"], "kind.python");

    let (status, body) = call(
        &mut app,
        Request::get("/entities?type=kind.python")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
}
