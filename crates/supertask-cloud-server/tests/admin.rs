use axum::{
    body::{to_bytes, Body},
    http::{Request, StatusCode},
};
use serde_json::{json, Value};
use sqlx::Row;
use tower::util::ServiceExt;

use supertask_cloud_server::{admin, app, auth, config::Config, state::AppState};

const ADMIN_EMAIL: &str = "root@supertask.invalid";
const ADMIN_PASSWORD: &str = "administrator-pass-1";
const MEMBER_EMAIL: &str = "member@supertask.invalid";
const MEMBER_PASSWORD: &str = "member-password-1";

async fn connect(config: Config) -> AppState {
    let state = AppState::connect(config).await.unwrap();
    auth::seed_account(&state.pool, MEMBER_EMAIL, MEMBER_PASSWORD)
        .await
        .unwrap();
    state
}

fn base_config(console_dir: &str) -> Config {
    Config {
        bind: "127.0.0.1:0".parse().unwrap(),
        database_url: ":memory:".into(),
        seed: false,
        seed_email: "seed@example.invalid".into(),
        seed_password: None,
        entities_max: 10,
        bytes_max: 100_000,
        admin_email: None,
        admin_password: None,
        console_dir: console_dir.into(),
    }
}

/// App with the admin bootstrap configured, mirroring the `main.rs` startup order.
async fn admin_app() -> axum::Router {
    admin_app_with_pool().await.0
}

async fn admin_app_with_pool() -> (axum::Router, sqlx::SqlitePool) {
    let mut config = base_config("does-not-exist/dist");
    config.admin_email = Some(ADMIN_EMAIL.into());
    config.admin_password = Some(ADMIN_PASSWORD.into());
    let state = connect(config.clone()).await;
    admin::bootstrap_admin(&state.pool, ADMIN_EMAIL, ADMIN_PASSWORD)
        .await
        .unwrap();
    (app(state.clone()), state.pool.clone())
}

/// App with no admin bootstrapped at all.
async fn headless_app() -> axum::Router {
    app(connect(base_config("does-not-exist/dist")).await)
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
        match serde_json::from_slice(&body) {
            Ok(parsed) => parsed,
            Err(_) => Value::String(String::from_utf8_lossy(&body).into_owned()),
        }
    };
    (status, value)
}

async fn bearer(app: &mut axum::Router, path: &str, email: &str, password: &str) -> String {
    let (status, body) = call(
        app,
        Request::post(path)
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": email, "password": password}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "login failed: {body}");
    body["access_token"].as_str().unwrap().to_string()
}

async fn admin_token(app: &mut axum::Router) -> String {
    bearer(app, "/admin/api/login", ADMIN_EMAIL, ADMIN_PASSWORD).await
}

fn get(path: &str, token: Option<&str>) -> Request<Body> {
    let mut builder = Request::get(path).body(Body::empty()).unwrap();
    if let Some(token) = token {
        builder
            .headers_mut()
            .insert("authorization", format!("Bearer {token}").parse().unwrap());
    }
    builder
}

fn put_json(path: &str, token: &str, payload: Value) -> Request<Body> {
    Request::put(path)
        .header("authorization", format!("Bearer {token}"))
        .header("content-type", "application/json")
        .body(Body::from(payload.to_string()))
        .unwrap()
}

async fn account_ids(app: &mut axum::Router, token: &str) -> Vec<String> {
    let (status, body) = call(app, get("/admin/api/accounts", Some(token))).await;
    assert_eq!(status, StatusCode::OK, "list failed: {body}");
    body.as_array()
        .unwrap()
        .iter()
        .map(|row| row["id"].as_str().unwrap().to_string())
        .collect()
}

#[tokio::test]
async fn admin_api_is_closed_to_non_admins() {
    let mut app = admin_app().await;

    // Setup probe needs no credential and exposes no account data.
    let (status, body) = call(&mut app, get("/admin/api/status", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["admin_available"], true);
    assert_eq!(body["console_ready"], false);
    assert!(body.get("email").is_none());

    let (status, body) = call(&mut app, get("/admin/api/me", None)).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "CLOUD_AUTH_FAILED");

    // A valid member session must not reach the admin surface.
    let member = bearer(&mut app, "/auth/login", MEMBER_EMAIL, MEMBER_PASSWORD).await;
    for path in ["/admin/api/me", "/admin/api/accounts"] {
        let (status, body) = call(&mut app, get(path, Some(&member))).await;
        assert_eq!(status, StatusCode::FORBIDDEN, "{path}");
        assert_eq!(body["code"], "ADMIN_FORBIDDEN", "{path}");
    }
    let (status, _) = call(
        &mut app,
        get("/admin/api/accounts/anything/role", Some(&member)),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    // Console login: right credentials, wrong role.
    let (status, body) = call(
        &mut app,
        Request::post("/admin/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": MEMBER_EMAIL, "password": MEMBER_PASSWORD}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "ADMIN_FORBIDDEN");

    // Wrong password and unknown email stay indistinguishable.
    let wrong = call(
        &mut app,
        Request::post("/admin/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": ADMIN_EMAIL, "password": "definitely-wrong-pass"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    let unknown = call(
        &mut app,
        Request::post("/admin/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": "ghost@supertask.invalid", "password": ADMIN_PASSWORD}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(wrong.0, StatusCode::UNAUTHORIZED);
    assert_eq!(wrong, unknown);
}

#[tokio::test]
async fn login_without_any_admin_reports_setup_code() {
    let mut app = headless_app().await;
    let (status, body) = call(
        &mut app,
        Request::post("/admin/api/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": MEMBER_EMAIL, "password": MEMBER_PASSWORD}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "ADMIN_NOT_CONFIGURED");

    let (status, body) = call(&mut app, get("/admin/api/status", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["admin_available"], false);
}

#[tokio::test]
async fn account_lifecycle_roles_and_password() {
    let mut app = admin_app().await;
    let token = admin_token(&mut app).await;
    let (status, me) = call(&mut app, get("/admin/api/me", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["email"], ADMIN_EMAIL);
    assert_eq!(me["role"], "admin");

    let created = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": "New.User@supertask.invalid", "password": MEMBER_PASSWORD})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(created.0, StatusCode::CREATED);
    let new_id = created.1["id"].as_str().unwrap().to_string();
    assert_eq!(created.1["email"], "new.user@supertask.invalid");
    assert_eq!(created.1["role"], "user");
    assert_eq!(created.1["disabled"], false);
    assert_eq!(created.1["entity_count"], 0);
    assert!(created.1.get("password").is_none());

    // Reuse of an existing email is refused.
    let (status, body) = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": MEMBER_EMAIL, "password": MEMBER_PASSWORD}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body["code"], "BAD_REQUEST");

    // Too-short and invalid-role payloads never reach the database.
    let (status, _) = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": "tiny@supertask.invalid", "password": "short"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    let (status, _) = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email": "role@supertask.invalid", "password": MEMBER_PASSWORD, "role": "superuser"})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);

    // The new account can sign in to the client API but not to the console.
    let member_token = bearer(
        &mut app,
        "/auth/login",
        "new.user@supertask.invalid",
        MEMBER_PASSWORD,
    )
    .await;
    let (status, body) = call(&mut app, get("/admin/api/accounts", Some(&member_token))).await;
    assert_eq!(status, StatusCode::FORBIDDEN);
    assert_eq!(body["code"], "ADMIN_FORBIDDEN");

    // Promote → console login works → demote → it stops working.
    let (status, body) = call(
        &mut app,
        put_json(
            &format!("/admin/api/accounts/{new_id}/role"),
            &token,
            json!({"role":"admin"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "admin");
    assert!(!admin_token(&mut app).await.is_empty());
    let promoted = bearer(
        &mut app,
        "/admin/api/login",
        "new.user@supertask.invalid",
        MEMBER_PASSWORD,
    )
    .await;
    assert!(!promoted.is_empty());

    let (status, body) = call(
        &mut app,
        put_json(
            &format!("/admin/api/accounts/{new_id}/role"),
            &token,
            json!({"role":"user"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["role"], "user");

    // Admin-set password replaces the credential; refresh rotation still requires admin.
    let (status, _) = call(
        &mut app,
        put_json(
            &format!("/admin/api/accounts/{new_id}/password"),
            &token,
            json!({"password":"rotated-password-9"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (status, _) = call(
        &mut app,
        Request::post("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"new.user@supertask.invalid","password":MEMBER_PASSWORD})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let rotated = bearer(
        &mut app,
        "/auth/login",
        "new.user@supertask.invalid",
        "rotated-password-9",
    )
    .await;
    assert!(!rotated.is_empty());

    // Disable blocks sign-in immediately; enable restores it.
    let (status, body) = call(
        &mut app,
        put_json(
            &format!("/admin/api/accounts/{new_id}/disabled"),
            &token,
            json!({"disabled":true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["disabled"], true);
    let (status, body) = call(
        &mut app,
        Request::post("/auth/login")
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"new.user@supertask.invalid","password":"rotated-password-9"})
                    .to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(body["code"], "CLOUD_AUTH_FAILED");
    let (status, _) = call(
        &mut app,
        get(&format!("/admin/api/accounts/{new_id}/role"), Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);

    let (status, body) = call(
        &mut app,
        get("/admin/api/accounts?query=new.user", Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_array().unwrap().len(), 1);
    let (status, body) = call(
        &mut app,
        get("/admin/api/accounts?query=nothing", Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(body.as_array().unwrap().is_empty());
}

#[tokio::test]
async fn self_service_guards_protect_the_operator() {
    let mut app = admin_app().await;
    let token = admin_token(&mut app).await;
    let (status, me) = call(&mut app, get("/admin/api/me", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    let own_id = me["account_id"].as_str().unwrap().to_string();

    for (path, payload) in [
        (
            format!("/admin/api/accounts/{own_id}/disabled"),
            json!({"disabled":true}),
        ),
        (
            format!("/admin/api/accounts/{own_id}/role"),
            json!({"role":"user"}),
        ),
    ] {
        let (status, body) = call(&mut app, put_json(&path, &token, payload)).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["code"], "BAD_REQUEST");
    }
    let (status, body) = call(
        &mut app,
        Request::delete(format!("/admin/api/accounts/{own_id}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body["error"].as_str().unwrap().contains("不能删除"));

    // A second admin can be demoted while at least one remains enabled.
    let (status, _) = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"second@supertask.invalid","password":MEMBER_PASSWORD,"role":"admin"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let ids = account_ids(&mut app, &token).await;
    let second = ids
        .iter()
        .find(|id| *id != &own_id)
        .cloned()
        .expect("second admin created");
    let (status, _) = call(
        &mut app,
        put_json(
            &format!("/admin/api/accounts/{second}/role"),
            &token,
            json!({"role":"user"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    // Unknown account ids are 404; malformed ids — including encoded traversal — are 400.
    let (status, body) = call(
        &mut app,
        put_json(
            "/admin/api/accounts/acct-missing/role",
            &token,
            json!({"role":"admin"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(body["code"], "NOT_FOUND");
    for path in [
        "/admin/api/accounts/%2e%2e%2f%2e%2e%2fetc/role",
        "/admin/api/accounts/bad!id/role",
    ] {
        let (status, body) = call(&mut app, put_json(path, &token, json!({"role":"admin"}))).await;
        assert_eq!(status, StatusCode::BAD_REQUEST, "{path}");
        assert_eq!(body["code"], "BAD_REQUEST", "{path}");
    }
}

#[tokio::test]
async fn deleting_a_disabled_admin_is_allowed_while_one_stays_enabled() {
    let mut app = admin_app().await;
    let token = admin_token(&mut app).await;
    let (status, created) = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"spare@supertask.invalid","password":MEMBER_PASSWORD,"role":"admin"}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let spare_id = created["id"].as_str().unwrap().to_string();

    // Disabling first must not make the account undeletable: it is not part of the
    // enabled-admin count that the last-operator guard protects.
    let (status, _) = call(
        &mut app,
        put_json(
            &format!("/admin/api/accounts/{spare_id}/disabled"),
            &token,
            json!({"disabled": true}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = call(
        &mut app,
        Request::delete(format!("/admin/api/accounts/{spare_id}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
}

#[tokio::test]
async fn deleting_an_account_cascades_every_child_row() {
    let (mut app, pool) = admin_app_with_pool().await;
    let token = admin_token(&mut app).await;
    let (status, body) = call(
        &mut app,
        Request::post("/admin/api/accounts")
            .header("authorization", format!("Bearer {token}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"email":"doomed@supertask.invalid","password":MEMBER_PASSWORD}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    let doomed_id = body["id"].as_str().unwrap().to_string();

    // Give the account real child rows through the public client API.
    let doomed = bearer(
        &mut app,
        "/auth/login",
        "doomed@supertask.invalid",
        MEMBER_PASSWORD,
    )
    .await;
    let (status, _) = call(
        &mut app,
        put_json(
            "/entities/ws-doomed",
            &doomed,
            json!({"type":"workspace","data":{"name":"doomed"},"base_rev":0,"updated_by":"device-1"}),
        ),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        &mut app,
        Request::post("/telemetry/batch")
            .header("authorization", format!("Bearer {doomed}"))
            .header("content-type", "application/json")
            .body(Body::from(
                json!({"events":[{"event":"app_start"}]}).to_string(),
            ))
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);

    let (status, rows) = call(
        &mut app,
        get("/admin/api/accounts?query=doomed", Some(&token)),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(rows.as_array().unwrap()[0]["entity_count"], 1);

    let (status, _) = call(
        &mut app,
        Request::delete(format!("/admin/api/accounts/{doomed_id}"))
            .header("authorization", format!("Bearer {token}"))
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);

    // The token minted before deletion must stop working at once.
    let (status, _) = call(&mut app, get("/quota", Some(&doomed))).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let (status, body) = call(&mut app, get("/admin/api/accounts", Some(&token))).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        body.as_array()
            .unwrap()
            .iter()
            .filter(|row| row["email"] == "doomed@supertask.invalid")
            .count(),
        0
    );

    // Every child row must be gone. `PRAGMA foreign_keys` is per-connection in SQLite,
    // so this is the regression guard for setting foreign_keys(true) on the pool's
    // connect options instead of running one pragma statement.
    assert_eq!(count(&pool, "SELECT COUNT(*) FROM accounts", None).await, 2);
    for (table, column) in [
        ("access_tokens", "account_id"),
        ("refresh_tokens", "account_id"),
        ("entities", "account_id"),
        ("telemetry_batches", "account_id"),
    ] {
        let rows = count(
            &pool,
            &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?"),
            Some(&doomed_id),
        )
        .await;
        assert_eq!(rows, 0, "cascade left rows in {table}");
    }
}

async fn count(pool: &sqlx::SqlitePool, sql: &str, account_id: Option<&str>) -> i64 {
    let mut query = sqlx::query(sql);
    if let Some(account_id) = account_id {
        query = query.bind(account_id);
    }
    query.fetch_one(pool).await.unwrap().get::<i64, _>(0)
}

#[tokio::test]
async fn console_assets_serve_and_refuse_traversal() {
    let dir = std::env::temp_dir().join(format!(
        "supertask-console-{}-{}",
        std::process::id(),
        // Distinct per test thread so parallel runs never share the fixture dir.
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.subsec_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(dir.join("assets")).unwrap();
    std::fs::write(dir.join("index.html"), b"<html>console</html>").unwrap();
    std::fs::write(dir.join("assets/app.js"), b"console.log(1)").unwrap();

    let mut config = base_config(dir.to_str().unwrap());
    config.admin_email = Some(ADMIN_EMAIL.into());
    config.admin_password = Some(ADMIN_PASSWORD.into());
    let state = connect(config).await;
    admin::bootstrap_admin(&state.pool, ADMIN_EMAIL, ADMIN_PASSWORD)
        .await
        .unwrap();
    let mut app = app(state);

    let (status, body) = call(&mut app, get("/admin/", None)).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body.as_str().unwrap(), "<html>console</html>");

    let response = app
        .clone()
        .oneshot(get("/admin/assets/app.js", None))
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/javascript; charset=utf-8"
    );

    let (status, _) = call(&mut app, get("/admin/assets/missing.js", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&mut app, get("/admin/%2e%2e/index.html", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
    let (status, _) = call(&mut app, get("/admin/Cargo.toml", None)).await;
    assert_eq!(status, StatusCode::NOT_FOUND);

    std::fs::remove_dir_all(&dir).ok();
}

#[tokio::test]
async fn unbuilt_console_falls_back_to_setup_notice() {
    let app = admin_app().await;
    let response = app.clone().oneshot(get("/admin/", None)).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers()["content-type"],
        "text/html; charset=utf-8"
    );
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("build:console"));
}
