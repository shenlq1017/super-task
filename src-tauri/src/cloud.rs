//! 2.0 云 IPC 命令层（v2.0 规格 §11，八条薄适配）。
//!
//! - provider 生产实例：HttpCloudProvider（端点可配置，appdata `cloud_endpoint`；
//!   缺省 `cloud::http::DEFAULT_ENDPOINT`——官方运营方是开放问题 #1，占位端点）；
//! - 会话/同步状态在 appdata `cloud/` 目录（CLI 同源，spec §14）；
//! - 本地存储绑定：settings（白名单）/ template（local 目录）/ workspace（yaml 落盘）。

use std::path::PathBuf;
use std::sync::{Mutex, RwLock};

use serde::Serialize;

use supertask_core::appdata::{self, AppData};
use supertask_core::cloud::http::HttpCloudProvider;
use supertask_core::cloud::migrate::{self, RestorePlan, ToolchainGap};
use supertask_core::cloud::sync::{load_state, save_state, LocalBinding, ResolveChoice, SyncState};
use supertask_core::cloud::telemetry::TelemetryBuffer;
use supertask_core::cloud::{CloudProvider, EntityData, EntityType, LoginTokens};
use supertask_core::error::{Error, ErrorCode, Result};

/// 壳层云运行时：可动态切换的 provider/端点 + 遥测缓冲。
///
/// provider 与 endpoint 总是成对替换，读请求只在调用期间持有读锁，保证
/// `cloud.endpoint.set` 生效后后续请求不会继续使用旧 provider。
#[derive(Debug, Clone, Default, Serialize)]
pub struct CloudRuntime {
    pub phase: String,
    pub last_attempt_ms: Option<u64>,
    pub last_success_ms: Option<u64>,
    pub last_error: Option<String>,
    pub last_result: Option<CloudRuntimeResult>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CloudRuntimeResult {
    pub pushed: usize,
    pub pulled: usize,
    pub pending: usize,
    pub skipped: usize,
    pub conflicts: usize,
}

pub struct CloudHandle {
    client: RwLock<CloudClient>,
    pub telemetry: Mutex<TelemetryBuffer>,
    runtime: Mutex<CloudRuntime>,
    operation_gate: Mutex<bool>,
}

struct OperationGuard<'a> {
    handle: &'a CloudHandle,
}
impl Drop for OperationGuard<'_> {
    fn drop(&mut self) {
        *self
            .handle
            .operation_gate
            .lock()
            .expect("cloud operation lock") = false;
    }
}

struct CloudClient {
    provider: Box<dyn CloudProvider>,
    endpoint: String,
}

impl CloudHandle {
    pub fn new(app: &AppData) -> Self {
        let endpoint = app
            .cloud_endpoint
            .as_deref()
            .and_then(|value| validate_endpoint(value).ok())
            .unwrap_or_else(|| supertask_core::cloud::http::DEFAULT_ENDPOINT.to_string());
        Self {
            client: RwLock::new(CloudClient {
                provider: Box::new(HttpCloudProvider::new(endpoint.clone())),
                endpoint,
            }),
            telemetry: Mutex::new(TelemetryBuffer::new(app.cloud_telemetry)),
            runtime: Mutex::new(CloudRuntime {
                phase: "idle".into(),
                ..Default::default()
            }),
            operation_gate: Mutex::new(false),
        }
    }

    fn begin_operation(&self) -> Result<OperationGuard<'_>> {
        let mut busy = self.operation_gate.lock().expect("cloud operation lock");
        if *busy {
            return Err(Error::new(
                ErrorCode::CloudProtocolError,
                "云端同步或迁移正在进行中",
            ));
        }
        *busy = true;
        let mut runtime = self.runtime.lock().expect("cloud runtime lock");
        runtime.phase = "syncing".into();
        runtime.last_attempt_ms = Some(now_ms());
        runtime.last_error = None;
        Ok(OperationGuard { handle: self })
    }

    fn finish_operation(&self, result: &Result<SyncOut>) {
        let mut runtime = self.runtime.lock().expect("cloud runtime lock");
        runtime.phase = "idle".into();
        match result {
            Ok(out) => {
                runtime.last_success_ms = Some(now_ms());
                runtime.last_error = None;
                runtime.last_result = Some(CloudRuntimeResult {
                    pushed: out.pushed,
                    pulled: out.pulled,
                    pending: out.pending.len(),
                    skipped: out.skipped.len(),
                    conflicts: out.conflicts.len(),
                });
            }
            Err(error) => runtime.last_error = Some(error.message().to_string()),
        }
    }

    pub fn endpoint(&self) -> String {
        self.client
            .read()
            .expect("cloud client lock")
            .endpoint
            .clone()
    }

    /// 严格校验并切换端点。调用方应先持久化 AppData，成功后再调用本方法。
    pub fn set_endpoint(&self, endpoint: &str) -> Result<String> {
        let endpoint = validate_endpoint(endpoint)?;
        let mut client = self.client.write().expect("cloud client lock");
        client.provider = Box::new(HttpCloudProvider::new(endpoint.clone()));
        client.endpoint = endpoint.clone();
        Ok(endpoint)
    }

    pub fn set_telemetry(&self, enabled: bool) {
        self.telemetry
            .lock()
            .expect("telemetry lock")
            .set_enabled(enabled);
    }

    fn session(&self) -> Result<LoginTokens> {
        supertask_core::cloud::session::load_session()
    }

    fn with_provider<T>(&self, f: impl FnOnce(&dyn CloudProvider) -> Result<T>) -> Result<T> {
        let client = self.client.read().expect("cloud client lock");
        f(&*client.provider)
    }

    /// Run one authenticated operation, refreshing and replaying exactly once
    /// when the current access token is rejected. A failed refresh invalidates
    /// the persisted session so callers cannot keep retrying a dead token.
    fn authenticated<T>(
        &self,
        mut operation: impl FnMut(&dyn CloudProvider, &str) -> Result<T>,
    ) -> Result<T> {
        let session = require_login(self)?;
        let first = self.with_provider(|provider| operation(provider, &session.access_token));
        match first {
            Ok(value) => Ok(value),
            Err(error) if error.code() == ErrorCode::CloudAuthFailed => {
                let refreshed =
                    self.with_provider(|provider| provider.refresh(&session.refresh_token));
                let refreshed = match refreshed {
                    Ok(tokens) => tokens,
                    Err(error) => {
                        let _ = supertask_core::cloud::session::clear_session();
                        return Err(Error::new(
                            ErrorCode::CloudAuthFailed,
                            format!("云端会话刷新失败: {}", error.message()),
                        ));
                    }
                };
                supertask_core::cloud::session::save_session(&refreshed)?;
                self.with_provider(|provider| operation(provider, &refreshed.access_token))
            }
            Err(error) => Err(error),
        }
    }
}

/// Cloud provider 端点只接受绝对 http/https URL，不接受 userinfo、空 host、
/// 空白或 query/fragment。路径可用于自托管服务，但不能改变 URL 的 authority。
fn validate_endpoint(endpoint: &str) -> Result<String> {
    let endpoint = endpoint.trim();
    if endpoint.is_empty()
        || endpoint != endpoint.trim()
        || endpoint.chars().any(char::is_whitespace)
    {
        return Err(Error::new(
            ErrorCode::CloudProtocolError,
            "云端点不能为空或含空白",
        ));
    }
    let (scheme, rest) = endpoint
        .split_once("://")
        .ok_or_else(|| Error::new(ErrorCode::CloudProtocolError, "云端点只允许 http/https"))?;
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Err(Error::new(
            ErrorCode::CloudProtocolError,
            "云端点只允许 http/https",
        ));
    }
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let authority = &rest[..authority_end];
    if authority.is_empty() {
        return Err(Error::new(ErrorCode::CloudProtocolError, "云端点缺少主机"));
    }
    if authority.contains('@') {
        return Err(Error::new(
            ErrorCode::CloudProtocolError,
            "云端点禁止 userinfo",
        ));
    }
    if rest[authority_end..].contains('?') || rest[authority_end..].contains('#') {
        return Err(Error::new(
            ErrorCode::CloudProtocolError,
            "云端点不允许 query 或 fragment",
        ));
    }
    if authority.ends_with(':') {
        return Err(Error::new(
            ErrorCode::CloudProtocolError,
            "云端点端口不能为空",
        ));
    }
    // Parse the authority without a URL dependency: host is required and the
    // optional port must be numeric. Bracketed IPv6 literals are supported.
    if authority.starts_with('[') {
        let close = authority
            .find(']')
            .ok_or_else(|| Error::new(ErrorCode::CloudProtocolError, "云端点主机无效"))?;
        if close == 1 {
            return Err(Error::new(ErrorCode::CloudProtocolError, "云端点主机无效"));
        }
        let suffix = &authority[close + 1..];
        if !suffix.is_empty() {
            let port = suffix
                .strip_prefix(':')
                .ok_or_else(|| Error::new(ErrorCode::CloudProtocolError, "云端点主机无效"))?;
            if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::new(ErrorCode::CloudProtocolError, "云端点端口无效"));
            }
        }
    } else {
        if authority.contains('[') || authority.contains(']') {
            return Err(Error::new(ErrorCode::CloudProtocolError, "云端点主机无效"));
        }
        let host = authority
            .rsplit_once(':')
            .map(|(h, _)| h)
            .unwrap_or(authority);
        if host.is_empty() || host.contains(':') {
            return Err(Error::new(ErrorCode::CloudProtocolError, "云端点主机无效"));
        }
        if let Some((_, port)) = authority.rsplit_once(':') {
            if port.is_empty() || !port.chars().all(|c| c.is_ascii_digit()) {
                return Err(Error::new(ErrorCode::CloudProtocolError, "云端点端口无效"));
            }
        }
    }
    Ok(endpoint.trim_end_matches('/').to_string())
}

fn require_login(handle: &CloudHandle) -> Result<LoginTokens> {
    handle.session()
}

// ---------------------------------------------------------------------------
// 本地存储绑定（LocalBinding 生产实现；各自独立读写，无共享借用）
// ---------------------------------------------------------------------------

/// 设置白名单（spec §6.1）：language / 通知开关 / 网络 app 默认。不含路径与密钥。
fn settings_whitelist(app: &AppData) -> serde_json::Value {
    serde_json::json!({
        "locale": app.locale,
        "log_notifications": app.log_notifications,
        "system_notifications": app.system_notifications,
        "network": {
            "proxy_mode": app.network.proxy_mode,
            "http": app.network.http,
            "https": app.network.https,
            "maven_mirror": app.network.maven_mirror,
            "npm_registry": app.network.npm_registry,
            "pip_index": app.network.pip_index,
            "go_goproxy": app.network.go_goproxy,
        },
    })
}

fn apply_settings_whitelist(app: &mut AppData, v: &serde_json::Value) {
    if let Some(s) = v.get("locale").and_then(|x| x.as_str()) {
        app.locale = s.to_string();
    }
    for (key, field) in [
        ("log_notifications", &mut app.log_notifications as *mut bool),
        (
            "system_notifications",
            &mut app.system_notifications as *mut bool,
        ),
    ] {
        if let Some(b) = v.get(key).and_then(|x| x.as_bool()) {
            unsafe { *field = b };
        }
    }
    if let Some(net) = v.get("network") {
        if let Some(s) = net.get("proxy_mode").and_then(|x| x.as_str()) {
            app.network.proxy_mode = s.to_string();
        }
        let opt = |k: &str| -> Option<String> {
            net.get(k).and_then(|x| x.as_str()).map(|s| s.to_string())
        };
        if let Some(s) = opt("http") {
            app.network.http = Some(s);
        }
        if let Some(s) = opt("https") {
            app.network.https = Some(s);
        }
        if let Some(s) = opt("maven_mirror") {
            app.network.maven_mirror = Some(s);
        }
        if let Some(s) = opt("npm_registry") {
            app.network.npm_registry = Some(s);
        }
        if let Some(s) = opt("pip_index") {
            app.network.pip_index = Some(s);
        }
        if let Some(s) = opt("go_goproxy") {
            app.network.go_goproxy = Some(s);
        }
    }
}

pub const SETTINGS_ID: &str = "app-settings";

fn appdata_path() -> PathBuf {
    appdata::appdata_dir().join("app.json")
}

/// settings 绑定：read/load 白名单；write = 应用白名单后整档保存。
struct SettingsBinding;
impl LocalBinding for SettingsBinding {
    fn entity_type(&self) -> EntityType {
        EntityType::Settings
    }
    fn ids(&self) -> Vec<String> {
        vec![SETTINGS_ID.to_string()]
    }
    fn read(&self, id: &str) -> Option<serde_json::Value> {
        (id == SETTINGS_ID).then(|| settings_whitelist(&appdata::load_at(&appdata_path())))
    }
    fn write(
        &mut self,
        id: &str,
        data: &serde_json::Value,
        _state: &mut SyncState,
    ) -> Result<bool> {
        if id != SETTINGS_ID {
            return Ok(false);
        }
        let mut app = appdata::load_at(&appdata_path());
        apply_settings_whitelist(&mut app, data);
        appdata::save_at(&appdata_path(), &app).map_err(|e| {
            Error::new(
                ErrorCode::Protocol,
                format!("设置保存失败: {}", e.message()),
            )
        })?;
        Ok(true)
    }
}

/// template 绑定：仅 local 来源模板（`%APPDATA%/SuperTask/templates/`）。
/// 首版只支持本地→云端推送；拉取落盘挂起（pending 报告，随 cloud.md 迭代补齐）。
struct TemplateBinding;
impl LocalBinding for TemplateBinding {
    fn entity_type(&self) -> EntityType {
        EntityType::Template
    }
    fn ids(&self) -> Vec<String> {
        local_template_ids()
    }
    fn read(&self, id: &str) -> Option<serde_json::Value> {
        read_template_entity(id)
    }
    fn write(
        &mut self,
        _id: &str,
        _data: &serde_json::Value,
        _state: &mut SyncState,
    ) -> Result<bool> {
        Ok(false)
    }
}

fn local_template_ids() -> Vec<String> {
    let dir = appdata::appdata_dir().join("templates");
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return vec![];
    };
    rd.flatten()
        .filter(|e| e.path().join("template.yaml").is_file())
        .filter_map(|e| {
            let text = std::fs::read_to_string(e.path().join("template.yaml")).ok()?;
            let v: serde_json::Value = serde_yaml::from_str(&text).ok()?;
            v.get("id")?.as_str().map(|s| s.to_string())
        })
        .collect()
}

fn read_template_entity(id: &str) -> Option<serde_json::Value> {
    let dir = appdata::appdata_dir().join("templates").join(id);
    let manifest = std::fs::read_to_string(dir.join("template.yaml")).ok()?;
    let v: serde_json::Value = serde_yaml::from_str(&manifest).ok()?;
    let mut files = serde_json::Map::new();
    if let Some(list) = v.get("files").and_then(|f| f.as_array()) {
        for f in list {
            if let Some(rel) = f.as_str() {
                if let Ok(content) = std::fs::read_to_string(dir.join(rel)) {
                    files.insert(rel.to_string(), serde_json::Value::String(content));
                }
            }
        }
    }
    Some(serde_json::json!({ "id": id, "files": files }))
}

/// workspace 绑定：state.json `local_path` 映射的实体。
/// read = `<dir>/supertask.yaml` 文本；write = pkg/import 语义（目标已有 yaml 拒绝、只落盘不启动）。
struct WorkspaceBinding;
impl LocalBinding for WorkspaceBinding {
    fn entity_type(&self) -> EntityType {
        EntityType::Workspace
    }
    fn ids(&self) -> Vec<String> {
        let state = load_state();
        state
            .entities
            .iter()
            .filter(|(_, t)| t.entity_type == EntityType::Workspace && t.local_path.is_some())
            .map(|(id, _)| id.clone())
            .collect()
    }
    fn read(&self, id: &str) -> Option<serde_json::Value> {
        let state = load_state();
        let path = state.entities.get(id)?.local_path.clone()?;
        let yaml =
            std::fs::read_to_string(std::path::Path::new(&path).join("supertask.yaml")).ok()?;
        let name = std::path::Path::new(&path)
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Some(serde_json::json!({ "name": name, "yaml": yaml }))
    }
    fn write(&mut self, id: &str, data: &serde_json::Value, state: &mut SyncState) -> Result<bool> {
        let Some(dir) = state.entities.get(id).and_then(|t| t.local_path.clone()) else {
            return Ok(false); // 未选目标目录 → 挂起（迁移向导里选）
        };
        let target = std::path::Path::new(&dir).join("supertask.yaml");
        if target.exists() {
            return Ok(false); // 目标已有 yaml → 拒绝（spec §6.2）
        }
        let yaml = data
            .get("yaml")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        std::fs::create_dir_all(&dir).ok();
        std::fs::write(&target, yaml)
            .map_err(|e| Error::new(ErrorCode::Protocol, format!("工作区落盘失败: {e}")))?;
        Ok(true)
    }
}

// ---------------------------------------------------------------------------
// 命令层逻辑（供 commands.rs 薄适配）
// ---------------------------------------------------------------------------

#[derive(Debug, Serialize)]
pub struct CloudStatusOut {
    pub logged_in: bool,
    pub email: Option<String>,
    pub device: String,
    pub endpoint: String,
    pub last_synced_ms: Option<u64>,
    pub conflicts: usize,
    pub conflict_ids: Vec<String>,
    pub telemetry_enabled: bool,
    pub quota: Option<supertask_core::cloud::QuotaUsage>,
    pub connection: String,
    pub health_detail: Option<String>,
    pub tracked: TrackedCounts,
    pub conflict_details: Vec<ConflictDetail>,
    pub telemetry_pending: usize,
    pub runtime: CloudRuntime,
}

#[derive(Debug, Serialize)]
pub struct TrackedCounts {
    pub total: usize,
    pub settings: usize,
    pub templates: usize,
    pub workspaces: usize,
    pub mapped_workspaces: usize,
}

#[derive(Debug, Serialize)]
pub struct ConflictDetail {
    pub id: String,
    pub entity_type: String,
    pub server_rev: u64,
    pub has_local: bool,
    pub has_server: bool,
}

#[derive(Debug, Serialize)]
pub struct CloudOkOut {
    pub ok: bool,
}

/// UI-facing login result; access/refresh tokens remain in the local session file.
#[derive(Debug, Serialize)]
pub struct CloudLoginOut {
    pub account_id: String,
    pub email: String,
    pub expires_in_secs: u64,
}

#[derive(Debug, Serialize)]
pub struct CloudEndpointOut {
    pub endpoint: String,
}

#[derive(Debug, Serialize)]
pub struct CloudTelemetryOut {
    pub enabled: bool,
}

#[derive(Debug, Serialize)]
pub struct SyncOut {
    pub pushed: usize,
    pub pulled: usize,
    pub pending: Vec<(String, String)>,
    pub skipped: Vec<String>,
    pub conflicts: Vec<String>,
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn tracked_counts(state: &SyncState) -> TrackedCounts {
    let mut c = TrackedCounts {
        total: state.entities.len(),
        settings: 0,
        templates: 0,
        workspaces: 0,
        mapped_workspaces: 0,
    };
    for tracked in state.entities.values() {
        match tracked.entity_type {
            EntityType::Settings => c.settings += 1,
            EntityType::Template => c.templates += 1,
            EntityType::Workspace => {
                c.workspaces += 1;
                if tracked.local_path.is_some() {
                    c.mapped_workspaces += 1;
                }
            }
            _ => {}
        }
    }
    c
}

fn conflict_details(state: &SyncState) -> Vec<ConflictDetail> {
    state
        .conflicts
        .iter()
        .map(|(id, c)| ConflictDetail {
            id: id.clone(),
            entity_type: c.entity_type.as_str().into(),
            server_rev: c.server_rev,
            has_local: c.local.is_some(),
            has_server: c.server.is_some(),
        })
        .collect()
}

pub fn cloud_status(handle: &CloudHandle) -> Result<CloudStatusOut> {
    let state = load_state();
    let logged_in = handle.session().is_ok();
    let mut health_detail = None;
    let quota = if logged_in {
        handle
            .authenticated(|provider, token| provider.quota(token))
            .map_err(|error| {
                health_detail = Some(error.message().to_string());
                error
            })
            .ok()
    } else {
        None
    };
    // A failed refresh clears the session; reflect that in the same status response.
    let session = handle.session().ok();
    let connection = if session.is_none() {
        "auth_required"
    } else if quota.is_some() {
        "online"
    } else {
        "offline"
    };
    let tracked = tracked_counts(&state);
    let conflict_details = conflict_details(&state);
    let telemetry_pending = handle.telemetry.lock().expect("telemetry lock").pending();
    let runtime = handle.runtime.lock().expect("cloud runtime lock").clone();
    Ok(CloudStatusOut {
        logged_in: session.is_some(),
        email: session.as_ref().map(|t| t.email.clone()),
        device: supertask_core::cloud::session::device_id(),
        endpoint: handle.endpoint(),
        last_synced_ms: state.last_synced_ms,
        conflicts: state.conflicts.len(),
        conflict_ids: state.conflicts.keys().cloned().collect(),
        telemetry_enabled: handle
            .telemetry
            .lock()
            .expect("telemetry lock")
            .is_enabled(),
        quota,
        connection: connection.into(),
        health_detail,
        tracked,
        conflict_details,
        telemetry_pending,
        runtime,
    })
}

pub fn cloud_login(handle: &CloudHandle, email: &str, password: &str) -> Result<LoginTokens> {
    let tokens = handle.with_provider(|provider| provider.login(email, password))?;
    supertask_core::cloud::session::save_session(&tokens)?;
    Ok(tokens)
}

pub fn cloud_logout() -> Result<()> {
    supertask_core::cloud::session::clear_session()
}

/// 校验、持久化并切换当前云端点。持久化失败时运行时 provider 不变。
pub fn cloud_endpoint_set(
    handle: &CloudHandle,
    appdata: &std::sync::Arc<Mutex<AppData>>,
    endpoint: &str,
) -> Result<CloudEndpointOut> {
    let endpoint = validate_endpoint(endpoint)?;
    let previous = {
        let mut app = appdata.lock().expect("appdata lock");
        let previous = app.cloud_endpoint.clone();
        app.cloud_endpoint = Some(endpoint.clone());
        if let Err(error) = appdata::save_at(&appdata_path(), &app) {
            app.cloud_endpoint = previous;
            return Err(error);
        }
        previous
    };
    if let Err(error) = handle.set_endpoint(&endpoint) {
        // Validation above makes this unreachable for the built-in provider;
        // retain the disk/runtime invariant if a future provider rejects it.
        let mut app = appdata.lock().expect("appdata lock");
        app.cloud_endpoint = previous;
        let _ = appdata::save_at(&appdata_path(), &app);
        return Err(error);
    }
    Ok(CloudEndpointOut { endpoint })
}

pub fn cloud_telemetry_set(
    handle: &CloudHandle,
    appdata: &std::sync::Arc<Mutex<AppData>>,
    enabled: bool,
) -> Result<CloudTelemetryOut> {
    {
        let mut app = appdata.lock().expect("appdata lock");
        let previous = app.cloud_telemetry;
        app.cloud_telemetry = enabled;
        if let Err(error) = appdata::save_at(&appdata_path(), &app) {
            app.cloud_telemetry = previous;
            return Err(error);
        }
    }
    handle.set_telemetry(enabled);
    Ok(CloudTelemetryOut { enabled })
}

pub fn cloud_sync(handle: &CloudHandle) -> Result<SyncOut> {
    let _gate = handle.begin_operation()?;
    let result = cloud_sync_inner(handle);
    handle.finish_operation(&result);
    result
}

fn cloud_sync_inner(handle: &CloudHandle) -> Result<SyncOut> {
    let mut state = load_state();
    let outcome = {
        let mut sb = SettingsBinding;
        let mut tb = TemplateBinding;
        let mut wb = WorkspaceBinding;
        let mut b: [&mut dyn LocalBinding; 3] = [&mut sb, &mut tb, &mut wb];
        handle.authenticated(|provider, token| {
            supertask_core::cloud::sync::sync(
                provider,
                token,
                &mut b,
                &mut state,
                &supertask_core::cloud::session::device_id(),
            )
        })?
    };
    state.last_synced_ms = Some(now_ms());
    save_state(&state)?;
    Ok(SyncOut {
        pushed: outcome.pushed,
        pulled: outcome.pulled,
        pending: outcome.pending,
        skipped: outcome.skipped,
        conflicts: state.conflicts.keys().cloned().collect(),
    })
}

pub fn cloud_resolve(handle: &CloudHandle, entity_id: &str, choice: &str) -> Result<SyncOut> {
    let choice = match choice {
        "local" => ResolveChoice::Local,
        "server" => ResolveChoice::Server,
        "both" => ResolveChoice::Both,
        _ => {
            return Err(Error::new(
                ErrorCode::Protocol,
                "choice 只允许 local | server | both",
            ))
        }
    };
    let mut state = load_state();
    {
        let mut sb = SettingsBinding;
        let mut tb = TemplateBinding;
        let mut wb = WorkspaceBinding;
        let mut b: [&mut dyn LocalBinding; 3] = [&mut sb, &mut tb, &mut wb];
        handle.authenticated(|provider, token| {
            supertask_core::cloud::sync::resolve(
                provider,
                token,
                &mut b,
                &mut state,
                entity_id,
                choice,
                &supertask_core::cloud::session::device_id(),
            )
        })?;
    }
    save_state(&state)?;
    Ok(SyncOut {
        pushed: 0,
        pulled: 0,
        pending: vec![],
        skipped: vec![],
        conflicts: state.conflicts.keys().cloned().collect(),
    })
}

/// 迁移 apply 入参：workspace 实体 → 落盘目录（spec §11）。
#[derive(Debug, Clone, serde::Deserialize)]
pub struct MigrateWorkspace {
    pub entity_id: String,
    pub dir: String,
}

/// 迁移 apply：选定落盘目录 → 走一次 sync（pull 落盘，目标已有 yaml 挂起）。
/// 工具链安装由前端逐项调用既有 toolchain.install（复用 operation 事件桥与取消）。
fn validate_migration_workspaces(workspaces: &[MigrateWorkspace]) -> Result<()> {
    let mut ids = std::collections::HashSet::new();
    for w in workspaces {
        if w.entity_id.trim().is_empty() || !ids.insert(w.entity_id.clone()) {
            return Err(Error::new(
                ErrorCode::CloudProtocolError,
                "迁移工作区实体 ID 为空或重复",
            ));
        }
        if w.dir.trim().is_empty() || !std::path::Path::new(&w.dir).is_absolute() {
            return Err(Error::new(
                ErrorCode::CloudProtocolError,
                "迁移工作区目录必须是非空绝对路径",
            ));
        }
    }
    Ok(())
}

pub fn cloud_migrate_apply(
    handle: &CloudHandle,
    workspaces: Vec<MigrateWorkspace>,
    include_templates: Option<bool>,
    include_settings: Option<bool>,
) -> Result<SyncOut> {
    let _gate = handle.begin_operation()?;
    let result = cloud_migrate_apply_inner(handle, workspaces, include_templates, include_settings);
    handle.finish_operation(&result);
    result
}

fn cloud_migrate_apply_inner(
    handle: &CloudHandle,
    workspaces: Vec<MigrateWorkspace>,
    include_templates: Option<bool>,
    include_settings: Option<bool>,
) -> Result<SyncOut> {
    validate_migration_workspaces(&workspaces)?;
    let mut state = load_state();
    for w in &workspaces {
        let tracked = state
            .entities
            .entry(w.entity_id.clone())
            .or_insert_with(|| supertask_core::cloud::sync::TrackedEntity {
                entity_type: EntityType::Workspace,
                base_rev: 0,
                last_synced_hash: String::new(),
                local_path: None,
            });
        tracked.local_path = Some(w.dir.clone());
    }
    let mut sb = SettingsBinding;
    let mut tb = TemplateBinding;
    let mut wb = WorkspaceBinding;
    let mut b: Vec<&mut dyn LocalBinding> = Vec::new();
    if include_settings.unwrap_or(true) {
        b.push(&mut sb);
    }
    if include_templates.unwrap_or(true) {
        b.push(&mut tb);
    }
    if !workspaces.is_empty() {
        b.push(&mut wb);
    }
    let outcome = handle.authenticated(|provider, token| {
        supertask_core::cloud::sync::sync(
            provider,
            token,
            &mut b,
            &mut state,
            &supertask_core::cloud::session::device_id(),
        )
    })?;
    state.last_synced_ms = Some(now_ms());
    save_state(&state)?;
    Ok(SyncOut {
        pushed: outcome.pushed,
        pulled: outcome.pulled,
        pending: outcome.pending,
        skipped: outcome.skipped,
        conflicts: state.conflicts.keys().cloned().collect(),
    })
}

pub fn cloud_migrate_plan(handle: &CloudHandle) -> Result<RestorePlan> {
    let entities = handle.authenticated(|provider, token| provider.list(token, None))?;
    let summaries: Vec<supertask_core::cloud::migrate::EntitySummary> = entities
        .iter()
        .map(|e| {
            let name = match &e.data {
                EntityData::Plain(v) => v
                    .get("name")
                    .or_else(|| v.get("title"))
                    .and_then(|x| x.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or(&e.id)
                    .to_string(),
                EntityData::Encrypted { .. } => e.id.clone(),
            };
            supertask_core::cloud::migrate::EntitySummary {
                id: e.id.clone(),
                entity_type: e.entity_type.as_str().into(),
                name,
            }
        })
        .collect();
    let mut gaps: Vec<ToolchainGap> = Vec::new();
    for e in &entities {
        if let EntityData::Plain(v) = &e.data {
            if let Some(yaml) = v.get("yaml").and_then(|x| x.as_str()) {
                if let Ok((spec, _)) = supertask_core::spec::parse_yaml(yaml) {
                    if let Some(tc) = spec.toolchain {
                        for gap in
                            migrate::toolchain_gaps(&tc, &supertask_core::probe::probe_toolchain())
                        {
                            if !gaps.contains(&gap) {
                                gaps.push(gap);
                            }
                        }
                    }
                }
            }
        }
    }
    Ok(RestorePlan {
        entities: summaries,
        toolchain_gaps: gaps,
    })
}
