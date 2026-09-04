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
        admin_email: None,
        admin_password: None,
        console_dir: "does-not-exist/dist".into(),
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
    assert_eq!(body["db"], "ok");
    assert!(body["now_ms"].as_u64().unwrap() > 0);
    assert!(body["version"].as_str().unwrap().len() > 0);

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
    assert_eq!(body["name"], "one");

    let (status, body) = call(&mut app, put(0, json!({"name":"stale"}))).await;
    assert_eq!(status, StatusCode::CONFLICT);
    assert_eq!(body["code"], "CLOUD_SYNC_CONFLICT");
    assert_eq!(body["current"]["rev"], 1);
    assert_eq!(body["current"]["data"]["name"], "one");
    assert_eq!(body["current"]["name"], "one");

    let (status, body) = call(
        &mut app,
        Request::get("/entities?type=workspace")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "one");
    assert!(list[0].get("data").is_some());
    assert_eq!(list[0]["data"]["name"], "one");
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
    assert_eq!(body["name"], "one");

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
    assert!(body["by_type"].as_array().unwrap().len() >= 1);
    assert_eq!(body["by_type"][0]["type"], "workspace");
    assert_eq!(body["by_type"][0]["entities"], 1);

    let (status, body) = call(
        &mut app,
        Request::post("/telemetry/batch")
            .header("authorization", &auth)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"events":[{"event":"app_start"},{"event":"feature_open","feature_id":"run"}]})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["accepted"], 2);

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
    // no name/title → name falls back to id
    assert_eq!(response.1["name"], "future-1");

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
    assert_eq!(body[0]["name"], "future-1");
}

#[tokio::test]
async fn entity_name_from_title_and_device_header_updated_by() {
    let mut app = test_app().await;
    let token = login(&mut app).await;
    let auth = format!("Bearer {token}");

    // title used when name missing
    let (status, body) = call(
        &mut app,
        Request::put("/entities/doc-1")
            .header("authorization", &auth)
            .header("content-type", "application/json")
            .header("x-device-id", "tablet-9")
            .body(Body::from(
                json!({
                    "type": "template",
                    "data": {"title": "My Template"},
                    "base_rev": 0
                })
                .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["name"], "My Template");
    assert_eq!(body["updated_by"], "tablet-9");
    assert_eq!(body["data"]["title"], "My Template");

    let (status, body) = call(
        &mut app,
        Request::get("/entities")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let list = body.as_array().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0]["name"], "My Template");
    assert!(list[0].get("data").is_some());
}

#[tokio::test]
async fn telemetry_policy_and_invalid_batch() {
    let mut app = test_app().await;
    let token = login(&mut app).await;
    let auth = format!("Bearer {token}");

    let (status, _) = call(
        &mut app,
        Request::get("/telemetry/policy")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(
        &mut app,
        Request::get("/telemetry/policy")
            .header("authorization", &auth)
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["enabled_by_default"], false);
    assert_eq!(body["retention"], "counts_only");
    assert_eq!(body["max_events_per_batch"], 256);
    assert_eq!(body["max_batch_bytes"], 262144);
    let events = body["events"].as_array().unwrap();
    assert!(events.iter().any(|e| e == "app_start"));

    let (status, _) = call(
        &mut app,
        Request::post("/telemetry/batch")
            .header("authorization", &auth)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"events":[{"event":"unknown_event"}]}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
}
