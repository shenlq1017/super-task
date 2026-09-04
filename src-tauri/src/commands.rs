//! Thin Tauri IPC over `supertask-core::Engine`.
//! Business logic lives in the engine; commands only (de)serialize + translate.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use indexmap::IndexMap;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};

use supertask_core::appdata::paths_equivalent;
use supertask_core::engine::ScriptState;
use supertask_core::error::ErrorCode;
use supertask_core::git;
use supertask_core::ide;
use supertask_core::importer;
use supertask_core::ipc::{IpcError, LogSource, LogSourceKind, PROTOCOL};
use supertask_core::merge;
use supertask_core::runtime::RtState;
use supertask_core::scan::{classify_scan_warning, scan_draft};
use supertask_core::template;
use supertask_core::Engine;
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_updater::UpdaterExt;

use crate::state;
use crate::state::{AppDataHandle, Exiting, HubHandle, PendingUpdate, TrayItems};

pub type EngineState<'a> = State<'a, Arc<Engine>>;
pub type HubState<'a> = State<'a, HubHandle>;
pub type AppDataRef<'a> = State<'a, AppDataHandle>;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// 长操作事件名（core 未定义常量，见 docs/spec/ipc.md §10.0）。
/// Tauri v2 事件名不允许点号，见 core ipc::event 模块注释。
const EVENT_OPERATION: &str = supertask_core::ipc::event::OPERATION;

/// 托盘图标 id（lib.rs build_tray 构建时指定）。
pub(crate) const TRAY_ID: &str = "main";

/// 退出中拦截（v1.1 规格 §8.3）：置位后拒绝新的启动/模板/Git 操作。
pub(crate) fn ensure_not_exiting(exiting: &Exiting) -> Result<(), IpcError> {
    if state::is_exiting(exiting) {
        Err(err(
            ErrorCode::AlreadyInProgress,
            "应用正在退出，已拒绝新操作",
        ))
    } else {
        Ok(())
    }
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn ipc_err(e: supertask_core::Error) -> IpcError {
    IpcError::from(&e)
}

pub(crate) fn err(code: ErrorCode, message: impl Into<String>) -> IpcError {
    IpcError::from(&supertask_core::Error::new(code, message))
}

/// 把 JSON 值反序列化成 operation result。
/// src-tauri 不直接依赖 serde_yaml（约定不改 Cargo.toml），目标类型
/// `serde_yaml::Value` 由 `hub.spawn` 闭包签名的返回类型推断，这里不点名。
fn json_into<T: serde::de::DeserializeOwned>(
    v: serde_json::Value,
) -> supertask_core::error::Result<T> {
    T::deserialize(v).map_err(|e| {
        supertask_core::Error::new(
            ErrorCode::Protocol,
            format!("构造 operation result 失败: {e}"),
        )
    })
}

fn warnings_to_strings(w: &[supertask_core::spec::ParseWarning]) -> Vec<String> {
    w.iter()
        .map(|p| {
            let code = error_code_str(p.code);
            format!("[{code}] {}", p.message)
        })
        .collect()
}

fn error_code_str(code: ErrorCode) -> String {
    serde_json::to_value(code)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| format!("{code:?}"))
}

/// Additive 结构化警告（旧客户端只读 `warnings: string[]`）。
#[derive(Debug, Clone, Serialize)]
pub struct WarningItem {
    pub code: String,
    pub message: String,
}

fn parse_warnings_to_items(w: &[supertask_core::spec::ParseWarning]) -> Vec<WarningItem> {
    w.iter()
        .map(|p| WarningItem {
            code: error_code_str(p.code),
            message: p.message.clone(),
        })
        .collect()
}

fn scan_warnings_to_items(warnings: &[String]) -> Vec<WarningItem> {
    warnings
        .iter()
        .map(|message| WarningItem {
            code: classify_scan_warning(message).to_string(),
            message: message.clone(),
        })
        .collect()
}

#[derive(Debug, serde::Deserialize)]
pub struct SourceArg {
    pub kind: String,
    pub id: String,
}

fn source_from_arg(a: Option<SourceArg>) -> Option<LogSource> {
    a.map(|s| {
        let kind = match s.kind.as_str() {
            "script" => LogSourceKind::Script,
            "system" => LogSourceKind::System,
            "gateway" => LogSourceKind::Gateway,
            _ => LogSourceKind::Service,
        };
        LogSource { kind, id: s.id }
    })
}

// ---------------------------------------------------------------------------
// Workspace
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct WorkspaceOpenOut {
    pub workspace_id: String,
    pub spec: supertask_core::spec::SuperTaskFile,
    pub warnings: Vec<String>,
    /// Additive：结构化警告；与 `warnings` 同序同文案，旧客户端可忽略。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warning_items: Vec<WarningItem>,
}

#[tauri::command(rename = "workspace.add")]
pub fn workspace_add(path: String) -> Result<WorkspaceOpenOut, IpcError> {
    let root = fs_canonicalize(&path)?;
    let (spec, warnings) = scan_draft(&root).map_err(ipc_err)?;
    let warning_items = scan_warnings_to_items(&warnings);
    Ok(WorkspaceOpenOut {
        workspace_id: root.to_string_lossy().into_owned(),
        spec,
        warnings,
        warning_items,
    })
}

#[tauri::command(rename = "workspace.open")]
pub fn workspace_open(
    state: EngineState<'_>,
    appdata: AppDataRef<'_>,
    app: AppHandle,
    path: String,
) -> Result<WorkspaceOpenOut, IpcError> {
    let root = fs_canonicalize(&path)?;
    // 1.7 §7：open 前注入 app 级网络默认（代理 + 镜像），启动链据此注入 env
    state.set_app_network(appdata.lock().expect("appdata lock").network.clone());
    let (warnings, _) = state.open(&root).map_err(ipc_err)?;
    let spec = state.spec().map_err(ipc_err)?;
    let workspace_id = state.workspace_id().map_err(ipc_err)?;
    {
        let mut data = appdata.lock().expect("appdata lock");
        data.record_open(&workspace_id);
    }
    // 回写失败不阻塞 open（下次启动仍可从 localStorage / 内存态恢复）
    let _ = state::save_appdata(&appdata);
    refresh_tray_from_engine(&app, &state);
    let warning_items = parse_warnings_to_items(&warnings);
    Ok(WorkspaceOpenOut {
        workspace_id,
        spec,
        warnings: warnings_to_strings(&warnings),
        warning_items,
    })
}

#[tauri::command(rename = "workspace.close")]
pub fn workspace_close(
    state: EngineState<'_>,
    app: AppHandle,
    workspace_id: String,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    let _ = workspace_id;
    state.close().map_err(ipc_err)?;
    refresh_tray_from_engine(&app, &state);
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

/// 切换工作区专用：不停进程，活服务移交后台注册表（重开同根工作区时接管）。
#[tauri::command(rename = "workspace.detach")]
pub fn workspace_detach(
    state: EngineState<'_>,
    app: AppHandle,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    state.detach().map_err(ipc_err)?;
    refresh_tray_from_engine(&app, &state);
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

#[tauri::command(rename = "workspace.forget")]
pub fn workspace_forget(
    state: EngineState<'_>,
    appdata: AppDataRef<'_>,
    app: AppHandle,
    path: Option<String>,
    // 兼容旧前端误传的 id（与 path 同义）
    id: Option<String>,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    let raw = path
        .or(id)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| err(ErrorCode::NotFound, "缺少 path"))?;
    // 尽量 canonicalize，便于与 workspace_id / recents 对齐；目录已删时退回原文。
    let key = fs_canonicalize(&raw)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_else(|_| raw.clone());

    // Spec：若仍打开则先 close；不删盘。
    if let Ok(cur) = state.workspace_id() {
        if paths_equivalent(&cur, &key) || paths_equivalent(&cur, &raw) {
            let _ = state.close();
        }
    }

    {
        let mut data = appdata.lock().expect("appdata lock");
        data.forget(&key);
        if key != raw {
            data.forget(&raw);
        }
    }
    state::save_appdata(&appdata).map_err(ipc_err)?;
    refresh_tray_from_engine(&app, &state);
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

#[tauri::command(rename = "workspace.scanDraft")]
pub fn workspace_scan_draft(path: String) -> Result<WorkspaceOpenOut, IpcError> {
    let root = fs_canonicalize(&path)?;
    let (spec, warnings) = scan_draft(&root).map_err(ipc_err)?;
    let warning_items = scan_warnings_to_items(&warnings);
    Ok(WorkspaceOpenOut {
        workspace_id: root.to_string_lossy().into_owned(),
        spec,
        warnings,
        warning_items,
    })
}

#[tauri::command(rename = "workspace.init")]
pub fn workspace_init(
    state: EngineState<'_>,
    appdata: AppDataRef<'_>,
    path: String,
    spec: supertask_core::spec::SuperTaskFile,
) -> Result<WorkspaceOpenOut, IpcError> {
    let root = fs_canonicalize(&path)?;
    let (warnings, _) = state.init(&root, spec).map_err(ipc_err)?;
    let spec = state.spec().map_err(ipc_err)?;
    let workspace_id = state.workspace_id().map_err(ipc_err)?;
    {
        let mut data = appdata.lock().expect("appdata lock");
        data.record_open(&workspace_id);
    }
    let _ = state::save_appdata(&appdata);
    let warning_items = parse_warnings_to_items(&warnings);
    Ok(WorkspaceOpenOut {
        workspace_id,
        spec,
        warnings: warnings_to_strings(&warnings),
        warning_items,
    })
}

/// 打开工作区目录（资源管理器）；`workspace.openExplorer` 命令与托盘菜单共用。
pub(crate) fn open_in_explorer(workspace_id: &str, rel: Option<&str>) -> Result<(), IpcError> {
    let root = PathBuf::from(workspace_id);
    if !root.is_dir() {
        return Err(err(
            ErrorCode::CwdMissing,
            format!("工作区目录不存在或无法访问: {}", root.display()),
        ));
    }
    let target = match rel {
        Some(r) => sandbox_join(&root, r)?,
        None => root,
    };
    if !target.exists() {
        return Err(err(
            ErrorCode::CwdMissing,
            format!("目标路径不存在: {}", target.display()),
        ));
    }
    #[cfg(windows)]
    {
        std::process::Command::new("explorer")
            .arg(&target)
            .spawn()
            .map_err(|e| {
                err(
                    ErrorCode::Spawn,
                    format!("打开资源管理器失败（{}）: {e}", target.display()),
                )
            })?;
    }
    #[cfg(not(windows))]
    {
        std::process::Command::new("xdg-open")
            .arg(&target)
            .spawn()
            .map_err(|e| {
                err(
                    ErrorCode::Spawn,
                    format!(
                        "打开文件管理器失败（{}）: {e}；请确认已安装 xdg-open",
                        target.display()
                    ),
                )
            })?;
    }
    Ok(())
}

#[tauri::command(rename = "workspace.openExplorer")]
pub fn workspace_open_explorer(
    _state: EngineState<'_>,
    workspace_id: String,
    rel: Option<String>,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    open_in_explorer(&workspace_id, rel.as_deref())?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

fn fs_canonicalize(path: &str) -> Result<PathBuf, IpcError> {
    // canonicalize 在 Windows 上返回 verbatim 路径（\\?\C:\…），剥掉前缀，
    // 避免 workspace_id 等进入 UI 的字符串带着机器内部表示。
    Ok(supertask_core::sandbox::strip_verbatim(
        std::fs::canonicalize(path).map_err(|e| {
            IpcError::from(&supertask_core::Error::new(
                ErrorCode::CwdMissing,
                format!("目录不存在或无法访问: {e}"),
            ))
        })?,
    ))
}

fn sandbox_join(root: &Path, rel: &str) -> Result<PathBuf, IpcError> {
    let candidate = if rel.is_empty() || rel == "." {
        root.to_path_buf()
    } else {
        root.join(rel)
    };
    let canon = supertask_core::sandbox::strip_verbatim(
        candidate
            .canonicalize()
            .unwrap_or_else(|_| candidate.clone()),
    );
    if !canon.starts_with(root) {
        return Err(IpcError::from(&supertask_core::Error::new(
            ErrorCode::PathEscape,
            "rel 逃出了工作区",
        )));
    }
    Ok(canon)
}

// ---------------------------------------------------------------------------
// YAML
// ---------------------------------------------------------------------------

/// 系统服务发现：列出正在监听端口的 java/node/python 等进程。
/// 端口表 / 进程快照读取失败显式报错（DISCOVER），不静默空列表。
#[tauri::command(rename = "system.discover")]
pub fn system_discover() -> Result<Vec<supertask_core::discover::ForeignService>, IpcError> {
    supertask_core::discover::discover_services().map_err(ipc_err)
}

#[derive(Serialize)]
pub struct KillProcessOut {
    pub ok: bool,
}

/// `system.killProcess`：终止发现列表中的监听进程（taskkill /T /F 杀整棵树）。
/// 护栏在 core：pid ≤ 4 / SuperTask 自身 / 当前无 LISTEN 端口一律拒绝。
#[tauri::command(rename = "system.killProcess")]
pub fn system_kill_process(pid: u32) -> Result<KillProcessOut, IpcError> {
    supertask_core::discover::kill_tree(pid).map_err(ipc_err)?;
    Ok(KillProcessOut { ok: true })
}

#[tauri::command(rename = "yaml.get")]
pub fn yaml_get(state: EngineState<'_>) -> Result<supertask_core::YamlView, IpcError> {
    state.yaml_get().map_err(ipc_err)
}

#[tauri::command(rename = "yaml.saveText")]
pub fn yaml_save_text(
    state: EngineState<'_>,
    _workspace_id: String,
    text: String,
    base_hash: String,
) -> Result<YamlSaveOut, IpcError> {
    let (spec, hash, warnings) = state.save_text(&text, &base_hash).map_err(ipc_err)?;
    Ok(YamlSaveOut {
        spec,
        hash,
        warnings: warnings_to_strings(&warnings),
    })
}

#[tauri::command(rename = "yaml.saveForm")]
pub fn yaml_save_form(
    state: EngineState<'_>,
    _workspace_id: String,
    spec: supertask_core::spec::SuperTaskFile,
    base_hash: String,
) -> Result<YamlSaveOut, IpcError> {
    let (spec, hash, warnings) = state.save_form(&spec, &base_hash).map_err(ipc_err)?;
    Ok(YamlSaveOut {
        spec,
        hash,
        warnings: warnings_to_strings(&warnings),
    })
}

#[derive(Serialize)]
pub struct YamlSaveOut {
    pub spec: supertask_core::spec::SuperTaskFile,
    pub hash: String,
    pub warnings: Vec<String>,
}

// ---------------------------------------------------------------------------
// Runtime
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct Accepted {
    pub accepted: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order: Option<Vec<String>>,
}

#[tauri::command(rename = "runtime.snapshot")]
pub fn runtime_snapshot(
    state: EngineState<'_>,
) -> Result<supertask_core::RuntimeSnapshot, IpcError> {
    state.snapshot().map_err(ipc_err)
}

#[tauri::command(rename = "runtime.startOne")]
pub fn runtime_start_one(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    _workspace_id: String,
    id: String,
) -> Result<Accepted, IpcError> {
    ensure_not_exiting(&exiting)?;
    state.start_one(&id).map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "runtime.startAll")]
pub fn runtime_start_all(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    _workspace_id: String,
) -> Result<Accepted, IpcError> {
    ensure_not_exiting(&exiting)?;
    let order = state.start_all().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: Some(order),
    })
}

#[tauri::command(rename = "runtime.stopOne")]
pub fn runtime_stop_one(
    state: EngineState<'_>,
    _workspace_id: String,
    id: String,
) -> Result<Accepted, IpcError> {
    state.stop_one(&id).map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "runtime.stopAll")]
pub fn runtime_stop_all(
    state: EngineState<'_>,
    _workspace_id: String,
) -> Result<Accepted, IpcError> {
    state.stop_all().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "runtime.restartOne")]
pub fn runtime_restart_one(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    _workspace_id: String,
    id: String,
) -> Result<Accepted, IpcError> {
    ensure_not_exiting(&exiting)?;
    state.restart_one(&id).map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

// ---------------------------------------------------------------------------
// Scripts
// ---------------------------------------------------------------------------

#[tauri::command(rename = "script.run")]
pub fn script_run(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    _workspace_id: String,
    id: String,
) -> Result<Accepted, IpcError> {
    ensure_not_exiting(&exiting)?;
    state.run_script(&id).map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "script.cancel")]
pub fn script_cancel(
    state: EngineState<'_>,
    _workspace_id: String,
    id: String,
) -> Result<Accepted, IpcError> {
    let _ = id;
    state.cancel_script().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

// ---------------------------------------------------------------------------
// Toolchain + prefs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ToolchainProbeOut {
    #[serde(flatten)]
    pub tools: supertask_core::probe::ToolchainProbe,
    /// 1.2：mise/winget 可用性（安装按钮态与 provider 元数据）。
    pub managers: supertask_core::toolchain::ManagerAvailability,
}

/// `toolchain.probe`：工具探测 + provider 元数据（§13.1）。
/// /env 深化 D1：会话内 TTL 缓存（Engine 侧），`refresh=true` 强制重探。
#[tauri::command(rename = "toolchain.probe")]
pub fn toolchain_probe(state: EngineState<'_>, refresh: Option<bool>) -> ToolchainProbeOut {
    let b = state.toolchain_probe(refresh.unwrap_or(false));
    ToolchainProbeOut {
        tools: b.tools,
        managers: b.managers,
    }
}

/// `toolchain.versions`：每工具可选版本列表（/env 深化 S1；winget 白名单 ∪ mise ls-remote 尾部）。
#[tauri::command(rename = "toolchain.versions")]
pub fn toolchain_versions(state: EngineState<'_>) -> supertask_core::ipc::ToolchainVersionsOutput {
    let mise = state.toolchain_probe(false).managers.mise;
    supertask_core::ipc::ToolchainVersionsOutput {
        tools: supertask_core::toolchain::versions::all_versions(
            &supertask_core::toolchain::ProcessRunner,
            mise,
        ),
    }
}

/// install/upgrade 共用内核：同步快速校验（非法输入立即拒绝，不发 operation），
/// 再入 hub 长操作跑安装 → 解析 →（可选）结构化写回 YAML。
#[allow(clippy::too_many_arguments)]
fn toolchain_spawn_op(
    state: EngineState<'_>,
    hub: HubState<'_>,
    appdata: AppDataRef<'_>,
    exiting: State<'_, Exiting>,
    tool: String,
    version: Option<String>,
    manager: Option<String>,
    persist: Option<bool>,
    base_hash: Option<String>,
    upgrade: bool,
) -> Result<OperationOut, IpcError> {
    use supertask_core::spec::ToolchainManager;
    use supertask_core::toolchain::{self, manifest};

    ensure_not_exiting(&exiting)?;
    let input = supertask_core::ipc::ToolchainInstallInput {
        tool,
        version,
        manager,
        persist: persist.unwrap_or(false),
        base_hash,
    };
    let tool = toolchain::parse_tool(&input.tool).map_err(ipc_err)?;
    let version = match &input.version {
        Some(v) => v.clone(),
        None => manifest::default_version(tool).to_string(),
    };
    toolchain::validate_version(&version).map_err(ipc_err)?;
    let requested = match input.manager.as_deref() {
        None => None,
        Some("auto") => Some(ToolchainManager::Auto),
        Some("mise") => Some(ToolchainManager::Mise),
        Some("winget") => Some(ToolchainManager::Winget),
        Some(other) => {
            return Err(err(
                ErrorCode::SpecInvalid,
                format!("未知 manager: {other}"),
            ))
        }
    };
    if input.persist && input.base_hash.is_none() {
        return Err(err(
            ErrorCode::SpecInvalid,
            "persist=true 必须携带 base_hash",
        ));
    }

    // 生效网络：workspace network 覆盖 app 默认（§7.2）；代理供 provider 下载使用
    let spec_view = if state.workspace_id().is_ok() {
        state.spec().ok()
    } else {
        None
    };
    let ws_manager = spec_view
        .as_ref()
        .and_then(|s| s.toolchain.as_ref())
        .and_then(|t| t.manager);
    let ws_network = spec_view.as_ref().and_then(|s| s.network.clone());
    let app_network = {
        let data = appdata.lock().expect("appdata lock");
        data.network.clone()
    };
    let env = supertask_core::network::tool_env(
        &supertask_core::network::resolve(ws_network.as_ref(), Some(&app_network))
            .map_err(ipc_err)?,
    )
    .map_err(ipc_err)?;

    let engine = state.inner().clone();
    let ws_root = state.workspace_id().ok().map(PathBuf::from);
    let persist = input.persist;
    let base_hash = input.base_hash.clone();
    let kind = if upgrade {
        "toolchain.upgrade"
    } else {
        "toolchain.install"
    };
    let op_id = hub.spawn(kind, move |ctx| {
        let root = ws_root
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let verb = if upgrade { "升级" } else { "安装" };
        ctx.report(None, format!("正在{verb} {} {version}", tool.as_str()));
        let req = toolchain::InstallRequest {
            tool,
            version: &version,
            requested,
            workspace_manager: ws_manager,
            workspace: &root,
            env,
            path_probe: supertask_core::probe::find_on_path,
        };
        let outcome = if upgrade {
            toolchain::upgrade(&toolchain::ProcessRunner, req)
        } else {
            toolchain::install(&toolchain::ProcessRunner, req)
        }?;
        // D1 失效点：安装/升级成功后探测结果已过时
        engine.invalidate_toolchain_probe();
        ctx.report(None, "安装完成，已解析工具路径");
        let mut result = serde_json::json!({
            "tool": outcome.tool.as_str(),
            "version": outcome.version,
            "manager": outcome.manager.as_str(),
            "path": outcome.resolved.program.to_string_lossy(),
        });
        if persist {
            // YAML_CONFLICT 时安装结果保留，仅写回失败（§4.3）
            let hash = base_hash.as_deref().expect("sync-checked above");
            let hash = persist_toolchain_version(&engine, tool, &outcome.version, hash)?;
            result["hash"] = serde_json::Value::String(hash);
        }
        Ok(json_into(result)?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

/// persist=true 时把版本要求写回 `toolchain`（npm/pnpm/yarn 写 `package_manager`）。
fn persist_toolchain_version(
    engine: &Engine,
    tool: supertask_core::toolchain::ToolKind,
    version: &str,
    base_hash: &str,
) -> supertask_core::error::Result<String> {
    use supertask_core::spec::{PackageManager, ToolchainSpec};
    use supertask_core::toolchain::ToolKind as K;
    let mut spec = engine.spec()?;
    let tc = spec.toolchain.get_or_insert_with(ToolchainSpec::default);
    match tool {
        K::Java => tc.java = Some(version.to_string()),
        K::Maven => tc.maven = Some(version.to_string()),
        K::Node => tc.node = Some(version.to_string()),
        K::Npm => tc.package_manager = Some(PackageManager::Npm),
        K::Pnpm => tc.package_manager = Some(PackageManager::Pnpm),
        K::Yarn => tc.package_manager = Some(PackageManager::Yarn),
        K::Bun => tc.package_manager = Some(PackageManager::Bun),
        // 1.7 §5：python/go 钉扎写回
        K::Python => tc.python = Some(version.to_string()),
        K::Go => tc.go = Some(version.to_string()),
    }
    let (_, hash, _) = engine.save_form(&spec, base_hash)?;
    Ok(hash)
}

/// 安装工具链（长操作，立即返回 operation_id；§13.1）。
#[tauri::command(rename = "toolchain.install")]
#[allow(clippy::too_many_arguments)]
pub fn toolchain_install(
    state: EngineState<'_>,
    hub: HubState<'_>,
    appdata: AppDataRef<'_>,
    exiting: State<'_, Exiting>,
    tool: String,
    version: Option<String>,
    manager: Option<String>,
    persist: Option<bool>,
    base_hash: Option<String>,
) -> Result<OperationOut, IpcError> {
    toolchain_spawn_op(
        state, hub, appdata, exiting, tool, version, manager, persist, base_hash, false,
    )
}

/// 升级工具链（长操作，立即返回 operation_id；§13.1）。
#[tauri::command(rename = "toolchain.upgrade")]
#[allow(clippy::too_many_arguments)]
pub fn toolchain_upgrade(
    state: EngineState<'_>,
    hub: HubState<'_>,
    appdata: AppDataRef<'_>,
    exiting: State<'_, Exiting>,
    tool: String,
    version: Option<String>,
    manager: Option<String>,
    persist: Option<bool>,
    base_hash: Option<String>,
) -> Result<OperationOut, IpcError> {
    toolchain_spawn_op(
        state, hub, appdata, exiting, tool, version, manager, persist, base_hash, true,
    )
}

#[tauri::command(rename = "app.savePrefs")]
pub fn app_save_prefs(
    app: AppHandle,
    appdata: AppDataRef<'_>,
    theme: Option<String>,
    restore_last: Option<bool>,
    close_to_tray: Option<bool>,
    start_on_login: Option<bool>,
    update_check: Option<bool>,
    locale: Option<String>,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    let mut autostart_change: Option<bool> = None;
    {
        let mut data = appdata.lock().expect("appdata lock");
        if let Some(v) = theme {
            data.theme = v;
        }
        if let Some(v) = restore_last {
            data.restore_last = v;
        }
        if let Some(v) = close_to_tray {
            data.close_to_tray = v;
        }
        if let Some(v) = start_on_login {
            data.start_on_login = v;
            autostart_change = Some(v);
        }
        if let Some(v) = update_check {
            data.update_check = v;
        }
        if let Some(v) = locale {
            // 1.4 §6.1：非法值由前端选择器约束；这里只透传，未知值 UI 回落 zh-CN
            data.locale = v;
        }
    }
    // 先落盘偏好，再注册/注销开机自启（v1.1 规格 §8.4）
    state::save_appdata(&appdata).map_err(ipc_err)?;

    if let Some(v) = autostart_change {
        // 只按需调用：系统注册状态与偏好一致时不重复操作，
        // 避免「未注册时 disable」在 Windows 注册表上误报失败。
        let result = apply_autostart(&app, v);
        if let Err(e) = result {
            // 回滚偏好为 false 并回写，UI 保持未开启状态
            {
                let mut data = appdata.lock().expect("appdata lock");
                data.start_on_login = false;
            }
            let _ = state::save_appdata(&appdata);
            return Err(err(
                ErrorCode::AutostartFailed,
                format!("开机启动注册失败: {e}"),
            ));
        }
    }
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

/// 把开机启动注册状态对齐到 `enable`；失败返回插件错误（由调用方映射 AUTOSTART_FAILED）。
fn apply_autostart(app: &AppHandle, enable: bool) -> Result<(), tauri_plugin_autostart::Error> {
    let mgr = app.autolaunch();
    let registered = mgr.is_enabled().unwrap_or(false);
    if enable == registered {
        return Ok(());
    }
    if enable {
        mgr.enable()
    } else {
        mgr.disable()
    }
}

/// 将文本写入用户选定路径（日志视图「下载」等）；路径由前端 save 对话框提供。
#[tauri::command(rename = "app.writeTextFile")]
pub fn app_write_text_file(
    path: String,
    contents: String,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    std::fs::write(&path, contents)
        .map_err(|e| err(ErrorCode::LogExportFailed, format!("写入文件失败: {e}")))?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

/// localStorage 一次性迁移：并入 recents / last_workspace 后落盘。
#[tauri::command(rename = "app.importRecents")]
pub fn app_import_recents(
    appdata: AppDataRef<'_>,
    recents: Vec<String>,
    last: Option<String>,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    {
        let mut data = appdata.lock().expect("appdata lock");
        data.merge_import(&recents, last.as_deref());
    }
    state::save_appdata(&appdata).map_err(ipc_err)?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

// ---------------------------------------------------------------------------
// Logs
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct LogSubOut {
    pub ok: bool,
    pub cursor: CursorOut,
}

#[derive(Serialize)]
pub struct CursorOut {
    pub next_seq: u64,
}

#[tauri::command(rename = "logs.subscribe")]
pub fn logs_subscribe(state: EngineState<'_>) -> Result<LogSubOut, IpcError> {
    let next = state.subscribe_logs().map_err(ipc_err)?;
    Ok(LogSubOut {
        ok: true,
        cursor: CursorOut { next_seq: next },
    })
}

#[tauri::command(rename = "logs.unsubscribe")]
pub fn logs_unsubscribe(state: EngineState<'_>) -> Result<HashMap<&'static str, bool>, IpcError> {
    state.unsubscribe_logs().map_err(ipc_err)?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

#[tauri::command(rename = "logs.snapshot")]
pub fn logs_snapshot(
    state: EngineState<'_>,
    source: Option<SourceArg>,
    limit: Option<usize>,
) -> Result<LogSnapshotOut, IpcError> {
    let src = source_from_arg(source);
    let (items, next_seq) = state
        .logs_snapshot(src.as_ref(), limit.unwrap_or(2000))
        .map_err(ipc_err)?;
    Ok(LogSnapshotOut { items, next_seq })
}

#[tauri::command(rename = "logs.clearView")]
pub fn logs_clear_view(
    state: EngineState<'_>,
    source: SourceArg,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    let src = match source.kind.as_str() {
        "script" => LogSourceKind::Script,
        "system" => LogSourceKind::System,
        "gateway" => LogSourceKind::Gateway,
        _ => LogSourceKind::Service,
    };
    state
        .clear_logs(&LogSource {
            kind: src,
            id: source.id,
        })
        .map_err(ipc_err)?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

#[derive(Serialize)]
pub struct LogSnapshotOut {
    pub items: Vec<supertask_core::log::LogLine>,
    pub next_seq: u64,
}

// ---------------------------------------------------------------------------
// 1.1 Templates / Git / IDE / 扫描合并
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct OperationOut {
    pub operation_id: String,
}

#[derive(Serialize)]
pub struct TemplatesListOut {
    pub templates: Vec<template::TemplateSummary>,
}

#[tauri::command(rename = "templates.list")]
pub fn templates_list() -> Result<TemplatesListOut, IpcError> {
    Ok(TemplatesListOut {
        templates: template::list_templates(state::templates_dir().as_deref()),
    })
}

/// 用模板（builtin/local）创建新工作区（长操作，立即返回 operation_id）。
#[tauri::command(rename = "templates.create")]
pub fn templates_create(
    hub: HubState<'_>,
    appdata: AppDataRef<'_>,
    exiting: State<'_, Exiting>,
    template_id: String,
    parent_path: String,
    directory_name: String,
    source: Option<String>,
    params: Option<std::collections::BTreeMap<String, String>>,
    blocks: Option<Vec<String>>,
    ports: Option<std::collections::BTreeMap<String, u32>>,
) -> Result<OperationOut, IpcError> {
    ensure_not_exiting(&exiting)?;
    // 同步快速校验：目录名非空（完整校验在 core 的 create_template 内）
    if directory_name.is_empty() {
        return Err(err(ErrorCode::PathEscape, "目录名不能为空"));
    }
    let source = match source.as_deref() {
        None | Some("builtin") => template::TemplateSourceKind::Builtin,
        Some("local") => template::TemplateSourceKind::Local,
        Some(other) => {
            return Err(err(
                ErrorCode::SpecInvalid,
                format!("未知模板来源: {other}"),
            ))
        }
    };
    let params: std::collections::BTreeMap<String, String> = params.unwrap_or_default();
    let local_dir = state::templates_dir();
    let appdata = appdata.inner().clone();
    let parent = PathBuf::from(&parent_path);
    let dir = directory_name;
    let op_id = hub.spawn("templates.create", move |_ctx| {
        let target = template::create_template(
            &template_id,
            source,
            &parent,
            &dir,
            local_dir.as_deref(),
            &params,
            blocks.as_deref(),
            &ports.unwrap_or_default(),
        )?;
        let ws = target.to_string_lossy().into_owned();
        {
            let mut data = appdata.lock().expect("appdata lock");
            data.record_open(&ws);
        }
        state::save_appdata(&appdata)?;
        Ok(json_into(serde_json::json!({ "workspace_id": ws }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

/// 组合模板预览（纯计算，无副作用）：组合选择 → 将生成的 services / 文件清单 / 警告。
#[tauri::command(rename = "templates.preview")]
pub fn templates_preview(
    template_id: String,
    source: Option<String>,
    blocks: Option<Vec<String>>,
    ports: Option<std::collections::BTreeMap<String, u32>>,
    params: Option<std::collections::BTreeMap<String, String>>,
) -> Result<template::TemplatePreviewOut, IpcError> {
    let source = match source.as_deref() {
        None | Some("builtin") => template::TemplateSourceKind::Builtin,
        Some("local") => template::TemplateSourceKind::Local,
        Some(other) => {
            return Err(err(
                ErrorCode::SpecInvalid,
                format!("未知模板来源: {other}"),
            ))
        }
    };
    Ok(template::preview_template(
        &template_id,
        source,
        state::templates_dir().as_deref(),
        blocks.as_deref(),
        &ports.unwrap_or_default(),
        &params.unwrap_or_default(),
    )
    .map_err(ipc_err)?)
}

/// clone 远端仓库为新工作区（长操作，立即返回 operation_id）。
#[tauri::command(rename = "git.clone")]
pub fn git_clone(
    hub: HubState<'_>,
    appdata: AppDataRef<'_>,
    url: String,
    target_path: String,
    branch: Option<String>,
) -> Result<OperationOut, IpcError> {
    // 同步快速校验：URL 不允许内嵌凭据
    git::check_url(&url).map_err(ipc_err)?;
    let appdata = appdata.inner().clone();
    let target = PathBuf::from(&target_path);
    let op_id = hub.spawn("git.clone", move |_ctx| {
        let canonical = git::clone(
            &git::ProcessRunner::default(),
            &url,
            &target,
            branch.as_deref(),
        )?;
        let ws = canonical.to_string_lossy().into_owned();
        {
            let mut data = appdata.lock().expect("appdata lock");
            data.record_open(&ws);
        }
        state::save_appdata(&appdata)?;
        Ok(json_into(serde_json::json!({ "workspace_id": ws }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

/// 校验 workspace_id 与引擎当前打开的工作区一致，否则 `NoWorkspace`。
fn require_current_workspace(state: &EngineState<'_>, workspace_id: &str) -> Result<(), IpcError> {
    let current = state.workspace_id().map_err(ipc_err)?;
    if current != workspace_id {
        return Err(err(
            ErrorCode::NoWorkspace,
            "workspace_id 与当前打开的工作区不一致",
        ));
    }
    Ok(())
}

#[tauri::command(rename = "git.status")]
pub fn git_status(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<git::GitStatus, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    git::status(
        &git::ProcessRunner::default(),
        &workspace_id,
        Path::new(&workspace_id),
    )
    .map_err(ipc_err)
}

/// 拉取远端更新（长操作）；服务/脚本运行中或有未提交修改时同步拒绝。
#[tauri::command(rename = "git.pull")]
pub fn git_pull(
    state: EngineState<'_>,
    hub: HubState<'_>,
    exiting: State<'_, Exiting>,
    workspace_id: String,
    remote: Option<String>,
    branch: Option<String>,
    allow_dirty: Option<bool>,
) -> Result<OperationOut, IpcError> {
    ensure_not_exiting(&exiting)?;
    require_current_workspace(&state, &workspace_id)?;

    // 有服务处于 starting/running/unhealthy/stopping 或脚本运行中 → 拒绝
    let snap = state.snapshot().map_err(ipc_err)?;
    let service_busy = snap.services.values().any(|s| {
        matches!(
            s.state,
            RtState::Starting
                | RtState::Running
                | RtState::Unhealthy
                | RtState::Stopping
                | RtState::Building
        )
    });
    let script_busy = snap
        .script
        .as_ref()
        .is_some_and(|s| s.state == ScriptState::Running);
    if service_busy || script_busy {
        return Err(err(
            ErrorCode::GitWorkspaceBusy,
            "有服务或脚本正在运行，停止后再拉取",
        ));
    }

    // dirty 且未显式允许 → 拒绝（不发 operation）
    let allow_dirty = allow_dirty.unwrap_or(false);
    let st = git::status(
        &git::ProcessRunner::default(),
        &workspace_id,
        Path::new(&workspace_id),
    )
    .map_err(ipc_err)?;
    if st.dirty && !allow_dirty {
        return Err(err(
            ErrorCode::GitDirty,
            "工作区有未提交修改，已阻止拉取；确认后可带 allow_dirty 重试",
        ));
    }

    let root = PathBuf::from(&workspace_id);
    let ws = workspace_id.clone();
    let op_id = hub.spawn("git.pull", move |_ctx| {
        git::pull(
            &git::ProcessRunner::default(),
            &root,
            remote.as_deref(),
            branch.as_deref(),
            allow_dirty,
        )?;
        Ok(json_into(serde_json::json!({ "workspace_id": ws }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

#[derive(Serialize)]
pub struct OpenIdeOut {
    pub accepted: bool,
    pub ide: ide::Ide,
    pub path: String,
}

/// 用指定 IDE 打开当前工作区根目录；返回命中 exe 仅展示用。
#[tauri::command(rename = "workspace.openIde")]
pub fn workspace_open_ide(
    state: EngineState<'_>,
    workspace_id: String,
    ide: String,
) -> Result<OpenIdeOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let parsed = ide::parse_ide(&ide).ok_or_else(|| {
        err(
            ErrorCode::NotFound,
            format!("未知 IDE: {ide}（支持 explorer|cursor|idea|code）"),
        )
    })?;
    let exe = ide::open(parsed, Path::new(&workspace_id)).map_err(ipc_err)?;
    Ok(OpenIdeOut {
        accepted: true,
        ide: parsed,
        path: exe.to_string_lossy().into_owned(),
    })
}

#[derive(Serialize)]
pub struct ScanPreviewOut {
    pub items: Vec<merge::ScanMergeItem>,
    pub warnings: Vec<String>,
}

/// 增量扫描预览：当前 spec 与重新发现的候选做可重复比对。
#[tauri::command(rename = "workspace.scanPreview")]
pub fn workspace_scan_preview(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<ScanPreviewOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let current = state.spec().map_err(ipc_err)?;
    let (discovered, warnings) = scan_draft(Path::new(&workspace_id)).map_err(ipc_err)?;
    let preview = merge::preview(&current, &discovered, warnings);
    Ok(ScanPreviewOut {
        items: preview.items,
        warnings: preview.warnings,
    })
}

/// 按用户选择应用扫描结果；写回走 saveForm 机制（base_hash 冲突 → YAML_CONFLICT）。
#[tauri::command(rename = "workspace.scanApply")]
pub fn workspace_scan_apply(
    state: EngineState<'_>,
    workspace_id: String,
    choices: Vec<merge::MergeChoice>,
    base_hash: String,
) -> Result<YamlSaveOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let current = state.spec().map_err(ipc_err)?;
    // 扫描可重复：应用前重新发现，与预览共用同一匹配规则
    let (discovered, _) = scan_draft(Path::new(&workspace_id)).map_err(ipc_err)?;
    let merged = merge::apply(&current, &discovered, &choices).map_err(ipc_err)?;
    let (spec, hash, warnings) = state.save_form(&merged, &base_hash).map_err(ipc_err)?;
    Ok(YamlSaveOut {
        spec,
        hash,
        warnings: warnings_to_strings(&warnings),
    })
}

// ---------------------------------------------------------------------------
// 2.1 README 导入（ipc.md §10.13）：scan 骨架 + README 草稿，走同一 merge 向导
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct ReadmePreviewOut {
    pub items: Vec<merge::ScanMergeItem>,
    pub script_items: Vec<merge::ScriptMergeItem>,
    pub warnings: Vec<String>,
    pub readme_path: Option<String>,
}

/// README 导入预览：确定性重导入 + 字段来源元数据（scan/readme + 置信度）。
#[tauri::command(rename = "import.readme")]
pub fn import_readme_preview(
    state: EngineState<'_>,
    workspace_id: String,
    path: Option<String>,
) -> Result<ReadmePreviewOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let current = state.spec().map_err(ipc_err)?;
    let imp = importer::readme::import_readme(Path::new(&workspace_id), path.as_deref())
        .map_err(ipc_err)?;
    let preview = merge::preview_with_sources(
        &current,
        &imp.draft,
        imp.warnings,
        Some((&imp.service_sources, &imp.script_sources)),
    );
    Ok(ReadmePreviewOut {
        items: preview.items,
        script_items: preview.script_items,
        warnings: preview.warnings,
        readme_path: imp.readme_path,
    })
}

/// README 导入应用：导入确定性可重复，应用前重导入后按选择合并；
/// 写回走 saveForm 机制（base_hash 冲突 → YAML_CONFLICT）。
#[tauri::command(rename = "import.readmeApply")]
pub fn import_readme_apply(
    state: EngineState<'_>,
    workspace_id: String,
    path: Option<String>,
    choices: Vec<merge::MergeChoice>,
    base_hash: String,
) -> Result<YamlSaveOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let current = state.spec().map_err(ipc_err)?;
    let imp = importer::readme::import_readme(Path::new(&workspace_id), path.as_deref())
        .map_err(ipc_err)?;
    let merged = merge::apply(&current, &imp.draft, &choices).map_err(ipc_err)?;
    let (spec, hash, warnings) = state.save_form(&merged, &base_hash).map_err(ipc_err)?;
    Ok(YamlSaveOut {
        spec,
        hash,
        warnings: warnings_to_strings(&warnings),
    })
}

// ---------------------------------------------------------------------------
// 1.4 Taskfile 导入（feature spec §7，ipc.md §10.8）
// ---------------------------------------------------------------------------

#[derive(Serialize)]
pub struct TaskfilePreviewOut {
    pub tasks: Vec<supertask_core::taskfile::TaskfileImportItem>,
    pub warnings: Vec<String>,
}

/// Taskfile v3 导入预览（纯内存计算，无落盘；文件缺失 `TASKFILE_NOT_FOUND`，
/// 版本/语法不支持 `TASKFILE_INVALID`）。
#[tauri::command(rename = "import.taskfilePreview")]
pub fn import_taskfile_preview(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<TaskfilePreviewOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let current = state.spec().map_err(ipc_err)?;
    let out = supertask_core::taskfile::preview(Path::new(&workspace_id), Some(&current.scripts))
        .map_err(ipc_err)?;
    Ok(TaskfilePreviewOut {
        tasks: out.tasks,
        warnings: out.warnings,
    })
}

/// 按选择应用 Taskfile 导入；写回走 saveForm 机制（base_hash 冲突 → `YAML_CONFLICT`）。
/// 只增改所选 `scripts.*`，其余字段不动。
#[tauri::command(rename = "import.taskfileApply")]
pub fn import_taskfile_apply(
    state: EngineState<'_>,
    workspace_id: String,
    selected: Vec<String>,
    base_hash: String,
) -> Result<YamlSaveOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let current = state.spec().map_err(ipc_err)?;
    let (merged, _) =
        supertask_core::taskfile::apply(&current, Path::new(&workspace_id), &selected)
            .map_err(ipc_err)?;
    let (spec, hash, warnings) = state.save_form(&merged, &base_hash).map_err(ipc_err)?;
    Ok(YamlSaveOut {
        spec,
        hash,
        warnings: warnings_to_strings(&warnings),
    })
}

// ---------------------------------------------------------------------------
// 1.1 Desktop: 自动更新（v1.1 规格 §9，ipc.md §10.6）
// ---------------------------------------------------------------------------

/// `app.update.check` 内核：构建 updater → check → 结果写 pending 供 install 消费。
/// 网络不可达 / manifest 不合法 / 配置错误统一映射 `UPDATE_FAILED`。
fn update_check_once(app: &AppHandle) -> supertask_core::error::Result<serde_yaml::Value> {
    let updater = app.updater_builder().build().map_err(|e| {
        supertask_core::Error::new(ErrorCode::UpdateFailed, format!("初始化更新器失败: {e}"))
    })?;
    match tauri::async_runtime::block_on(updater.check()) {
        Ok(None) => Ok(json_into(serde_json::json!({ "status": "up_to_date" }))?),
        Ok(Some(update)) => {
            // 先复制展示字段（json! 按值取用），Update 整体存入 pending 供 install 消费
            let payload = serde_json::json!({
                "status": "available",
                "version": update.version.clone(),
                "notes": update.body.clone(),
                "date": update.date.map(|d| d.to_string()),
            });
            let pending = app.state::<PendingUpdate>();
            *pending.lock().expect("pending update lock") = Some(update);
            Ok(json_into(payload)?)
        }
        Err(e) => Err(supertask_core::Error::new(
            ErrorCode::UpdateFailed,
            format!("检查更新失败: {e}"),
        )),
    }
}

/// 更新检查入口：`app.update.check` 命令与启动时静默检查共用。
/// 检查不修改工作区，允许与其它 operation 并发（结果经 `st.operation` 事件流送达）。
pub(crate) fn spawn_update_check(app: AppHandle, silent: bool) -> String {
    let hub = app.state::<HubHandle>().inner().clone();
    hub.spawn("app.update", move |_ctx| match update_check_once(&app) {
        Ok(result) => Ok(result),
        Err(e) => {
            if silent {
                eprintln!("[supertask] 启动时检查更新失败（已忽略）: {e}");
            }
            Err(e)
        }
    })
}

/// 检查更新（长操作，立即返回 operation_id）。
#[tauri::command(rename = "app.update.check")]
pub fn app_update_check(
    app: AppHandle,
    exiting: State<'_, Exiting>,
) -> Result<OperationOut, IpcError> {
    ensure_not_exiting(&exiting)?;
    let operation_id = spawn_update_check(app, false);
    Ok(OperationOut { operation_id })
}

/// 下载并安装更新（长操作）。Windows 上 install 拉起安装器后进程退出，其后代码不可达。
#[tauri::command(rename = "app.update.install")]
pub fn app_update_install(
    app: AppHandle,
    hub: HubState<'_>,
    engine: EngineState<'_>,
    exiting: State<'_, Exiting>,
    version: String,
) -> Result<OperationOut, IpcError> {
    ensure_not_exiting(&exiting)?;

    // 1.4 §4.5：Linux 只支持检查更新，安装返回 PLATFORM_UNSUPPORTED 并给手动替换指引
    #[cfg(not(windows))]
    {
        let _ = version;
        return Err(err(
            ErrorCode::PlatformUnsupported,
            "Linux 暂不支持应用内安装更新：请下载新版本 AppImage 手动替换",
        ));
    }
    #[cfg(windows)]
    {
        install_update_windows(app, hub, engine, version)
    }
}

#[cfg(windows)]
fn install_update_windows(
    app: AppHandle,
    hub: HubState<'_>,
    engine: EngineState<'_>,
    version: String,
) -> Result<OperationOut, IpcError> {
    // 前置 1：Git/模板/扫描等 operation 进行中 → 阻止（§9.2）
    if hub.has_active() {
        return Err(err(
            ErrorCode::UpdateBlockedRunning,
            "有其他操作正在进行，请稍后再试",
        ));
    }

    // 前置 2：服务 starting/running/unhealthy/stopping 或脚本运行中 → 阻止并给出下一步（§9.2）
    if let Ok(snap) = engine.snapshot() {
        let service_busy = snap.services.values().any(|s| {
            matches!(
                s.state,
                RtState::Starting
                    | RtState::Running
                    | RtState::Unhealthy
                    | RtState::Stopping
                    | RtState::Building
            )
        });
        let script_busy = snap
            .script
            .as_ref()
            .is_some_and(|s| s.state == ScriptState::Running);
        if service_busy || script_busy {
            return Err(err(
                ErrorCode::UpdateBlockedRunning,
                "服务或脚本仍在运行，请先停止全部服务再安装更新",
            ));
        }
    }

    // 前置 3：必须存在版本匹配的可用更新（来自此前 app.update.check）
    let pending = app.state::<PendingUpdate>();
    let mut guard = pending.lock().expect("pending update lock");
    if !guard.as_ref().is_some_and(|u| u.version == version) {
        drop(guard);
        return Err(err(
            ErrorCode::UpdateFailed,
            "没有可安装的更新，请先检查更新",
        ));
    }
    let update = guard.take().expect("checked above");
    drop(guard);

    let op_id = hub.spawn("app.update", move |ctx| {
        let reporter = ctx.clone();
        let mut downloaded: u64 = 0;
        let bytes = tauri::async_runtime::block_on(update.download(
            move |chunk, total| {
                downloaded += chunk as u64;
                let progress = total
                    .filter(|t| *t > 0)
                    .map(|t| (downloaded as f64 / t as f64).clamp(0.0, 1.0));
                reporter.report(progress, "正在下载更新");
            },
            || {},
        ))
        .map_err(|e| {
            supertask_core::Error::new(ErrorCode::UpdateFailed, format!("下载更新失败: {e}"))
        })?;
        ctx.report(None, "正在安装更新，应用即将退出");
        // Windows：install 拉起安装器并 std::process::exit(0)，其后的代码不可达；
        // 早期失败时把更新放回 pending，用户可重试安装。
        if let Err(e) = update.install(&bytes) {
            let pending = app.state::<PendingUpdate>();
            *pending.lock().expect("pending update lock") = Some(update);
            return Err(supertask_core::Error::new(
                ErrorCode::UpdateFailed,
                format!("安装更新失败: {e}"),
            ));
        }
        Ok(json_into(serde_json::json!({ "status": "installed" }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

// ---------------------------------------------------------------------------
// FEATURE_SOON placeholders
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 1.3 Docker（core 侧实现；这里只做 IPC 适配）
// ---------------------------------------------------------------------------

/// `system.metrics`：主机级 CPU / 内存 / 磁盘 / CPU 温度采样（状态栏用）。
/// 与工作区 Job 树指标（`metrics.snapshot`）口径不同：这里是整机视角。
/// 不持久化、不进日志、不上传。
///
/// `temp` 决定温度采样档位（`off` / `auto` / `fast`），由状态栏切换：
/// Windows 上 `fast` 会常驻一个采样进程，`auto` 每分钟查一次。
#[tauri::command(rename = "system.metrics")]
pub fn system_metrics(
    temp: Option<supertask_core::host_metrics::TempMode>,
) -> supertask_core::host_metrics::HostMetrics {
    supertask_core::host_metrics::sample_host_metrics(temp.unwrap_or_default())
}

/// `docker.probe`：三态探测，会话内缓存，`refresh` 强制刷新（规格 §4.1）。
#[tauri::command(rename = "docker.probe")]
pub fn docker_probe(
    state: EngineState<'_>,
    refresh: Option<bool>,
) -> Result<supertask_core::docker::DockerProbe, IpcError> {
    Ok(state.docker_probe(refresh.unwrap_or(false)))
}

/// `docker.ps`：当前 compose project 的容器列表（只读；无 compose 文件则空）。
#[tauri::command(rename = "docker.ps")]
pub fn docker_ps(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<supertask_core::ipc::DockerPsOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let containers = state.docker_ps().map_err(ipc_err)?;
    Ok(supertask_core::ipc::DockerPsOutput { containers })
}

/// `docker.images`：本机镜像只读列表（无缓存承诺）。
#[tauri::command(rename = "docker.images")]
pub fn docker_images(
    state: EngineState<'_>,
) -> Result<supertask_core::ipc::DockerImagesOutput, IpcError> {
    let images = state.docker_images().map_err(ipc_err)?;
    Ok(supertask_core::ipc::DockerImagesOutput { images })
}

/// `docker.build`：构建 `docker.builds` 中已定义条目（长操作，可取消，规格 §6）。
#[tauri::command(rename = "docker.build")]
pub fn docker_build(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    workspace_id: String,
    name: String,
) -> Result<OperationOut, IpcError> {
    ensure_not_exiting(&exiting)?;
    require_current_workspace(&state, &workspace_id)?;
    let operation_id = state.docker_build(&name).map_err(ipc_err)?;
    Ok(OperationOut { operation_id })
}

/// `docker.buildCancel`：best effort 取消构建（已提交层缓存不回滚）。
#[tauri::command(rename = "docker.buildCancel")]
pub fn docker_build_cancel(
    state: EngineState<'_>,
    workspace_id: String,
    operation_id: String,
) -> Result<serde_json::Value, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let ok = state.cancel_operation(&operation_id);
    Ok(serde_json::json!({ "ok": ok }))
}

// 1.6：gateway.apply 已转正为真命令；2.0：cloud 命令已转正；2.1：ai 命令已转正（见文件末尾命令组）

// ---------------------------------------------------------------------------
// 1.2 phase 3–7：端口 / secrets / 日志 / 指标 / profile / runtime.build
//（core 侧实现；这里只做 IPC 适配）
// ---------------------------------------------------------------------------

/// `ports.inspect`：端口占用 + 引擎托管判定（§5.1）。传 `port` 时只检查该候选端口，
/// 否则检查全部已配置端口。
#[tauri::command(rename = "ports.inspect")]
pub fn ports_inspect(
    state: EngineState<'_>,
    workspace_id: String,
    id: String,
    port: Option<u16>,
) -> Result<supertask_core::ipc::PortsInspectOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let items = state.ports_inspect(&id, port).map_err(ipc_err)?;
    Ok(supertask_core::ipc::PortsInspectOutput { items })
}

/// `env.effective`：服务最近一次启动实际注入的生效环境快照（引擎自报）。
#[tauri::command(rename = "env.effective")]
pub fn env_effective(
    state: EngineState<'_>,
    workspace_id: String,
    id: String,
) -> Result<supertask_core::ipc::EnvEffectiveOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.env_effective(&id).map_err(ipc_err)
}

/// `spring.inspect`：spring-boot 服务的项目自身配置静态解析（只读，不写回）。
#[tauri::command(rename = "spring.inspect")]
pub fn spring_inspect(
    state: EngineState<'_>,
    workspace_id: String,
    id: String,
) -> Result<supertask_core::spring::SpringConfigOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.spring_inspect(&id).map_err(ipc_err)
}

/// `ports.suggest`：建议端口候选（§5.2）。
#[tauri::command(rename = "ports.suggest")]
pub fn ports_suggest(
    state: EngineState<'_>,
    workspace_id: String,
    id: String,
) -> Result<supertask_core::ipc::PortsSuggestOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let candidates = state.ports_suggest(&id).map_err(ipc_err)?;
    Ok(supertask_core::ipc::PortsSuggestOutput { candidates })
}

/// `ports.assign`：一键改端口；运行中未确认 restart 时只预览（§5.3/§5.4）。
#[tauri::command(rename = "ports.assign")]
pub fn ports_assign(
    state: EngineState<'_>,
    workspace_id: String,
    id: String,
    port: u16,
    base_hash: String,
    restart: Option<bool>,
) -> Result<supertask_core::ipc::PortsAssignOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let view = state
        .ports_assign(&id, port, &base_hash, restart.unwrap_or(false))
        .map_err(ipc_err)?;
    Ok(supertask_core::ipc::PortsAssignOutput {
        operation_id: None,
        spec: serde_yaml::to_value(&view.spec)
            .map_err(|e| err(ErrorCode::Protocol, format!("序列化 spec 失败: {e}")))?,
        hash: view.hash,
        restart_required: view.restart_required,
        notes: view.notes,
    })
}

/// `secrets.status`：只返回 key 名与状态，绝不返回值（§6.4）。
#[tauri::command(rename = "secrets.status")]
pub fn secrets_status(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<supertask_core::ipc::SecretsStatusOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.secrets_status().map_err(ipc_err)
}

/// `secrets.set`：值只在本次 IPC 中传输；core 不落日志/事件。
#[tauri::command(rename = "secrets.set")]
pub fn secrets_set(
    state: EngineState<'_>,
    workspace_id: String,
    key: String,
    value: String,
) -> Result<supertask_core::ipc::SecretsKeyOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.secrets_set(&key, &value).map_err(ipc_err)?;
    Ok(supertask_core::ipc::SecretsKeyOutput { ok: true, key })
}

/// `secrets.delete`。
#[tauri::command(rename = "secrets.delete")]
pub fn secrets_delete(
    state: EngineState<'_>,
    workspace_id: String,
    key: String,
) -> Result<supertask_core::ipc::SecretsKeyOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.secrets_delete(&key).map_err(ipc_err)?;
    Ok(supertask_core::ipc::SecretsKeyOutput { ok: true, key })
}

/// `secrets.validate`：required 缺失只列 key 名（§6.4）。
#[tauri::command(rename = "secrets.validate")]
pub fn secrets_validate(
    state: EngineState<'_>,
    workspace_id: String,
    id: Option<String>,
) -> Result<supertask_core::ipc::SecretsValidateOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.secrets_validate(id.as_deref()).map_err(ipc_err)
}

/// `logs.search`（长操作）：literal 搜索历史文件（§8.3）。
#[tauri::command(rename = "logs.search")]
pub fn logs_search(
    state: EngineState<'_>,
    hub: HubState<'_>,
    workspace_id: String,
    source: Option<SourceArg>,
    query: String,
    case_sensitive: Option<bool>,
    limit: Option<u32>,
) -> Result<OperationOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let src = source_from_arg(source);
    let case = case_sensitive.unwrap_or(false);
    let engine = state.inner().clone();
    let op_id = hub.spawn("logs.search", move |_ctx| {
        let r = engine.search_logs(src.as_ref(), &query, case, limit.map(|l| l as usize))?;
        Ok(json_into(serde_json::json!({
            "items": r.items,
            "truncated": r.truncated,
            "files_scanned": r.files_scanned,
        }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

/// `logs.export`（长操作）：text/jsonl 导出，不覆盖已有文件（§8.4）。
/// destination 应来自 native save dialog（前端负责），core 只做存在性/父目录校验。
#[tauri::command(rename = "logs.export")]
pub fn logs_export(
    state: EngineState<'_>,
    hub: HubState<'_>,
    workspace_id: String,
    source: Option<SourceArg>,
    query: Option<String>,
    case_sensitive: Option<bool>,
    format: String,
    destination_path: String,
) -> Result<OperationOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let src = source_from_arg(source);
    let case = case_sensitive.unwrap_or(false);
    let dest = PathBuf::from(&destination_path);
    let engine = state.inner().clone();
    let op_id = hub.spawn("logs.export", move |_ctx| {
        let n = engine.export_logs(src.as_ref(), query.as_deref(), case, &format, &dest)?;
        Ok(json_into(
            serde_json::json!({ "count": n, "destination": dest.to_string_lossy() }),
        )?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

/// `logs.retention.run`（长操作）：按顶层 log_retention 清理轮转文件（§8.2）。
#[tauri::command(rename = "logs.retention.run")]
pub fn logs_retention_run(
    state: EngineState<'_>,
    hub: HubState<'_>,
    workspace_id: String,
) -> Result<OperationOut, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let engine = state.inner().clone();
    let op_id = hub.spawn("logs.retention.run", move |_ctx| {
        let s = engine.run_log_retention()?;
        Ok(json_into(serde_json::json!({
            "deleted_files": s.deleted_files,
            "deleted_bytes": s.deleted_bytes,
        }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

/// `metrics.snapshot`：最近一次采样（§9.2）。
#[tauri::command(rename = "metrics.snapshot")]
pub fn metrics_snapshot(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<supertask_core::ipc::MetricsSnapshotOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.metrics_snapshot().map_err(ipc_err)
}

#[tauri::command(rename = "metrics.subscribe")]
pub fn metrics_subscribe(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.metrics_subscribe().map_err(ipc_err)?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

#[tauri::command(rename = "metrics.unsubscribe")]
pub fn metrics_unsubscribe(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<HashMap<&'static str, bool>, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.metrics_unsubscribe().map_err(ipc_err)?;
    let mut m = HashMap::new();
    m.insert("ok", true);
    Ok(m)
}

/// `profiles.list`：active + 各 profile enabled 计数（§10）。
#[tauri::command(rename = "profiles.list")]
pub fn profiles_list(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<supertask_core::ipc::ProfilesListOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.profiles_list().map_err(ipc_err)
}

/// `profiles.activate`：忙则 PROFILE_SWITCH_BUSY；base_hash 结构化保存（§10.2）。
#[tauri::command(rename = "profiles.activate")]
pub fn profiles_activate(
    state: EngineState<'_>,
    workspace_id: String,
    id: String,
    base_hash: String,
) -> Result<supertask_core::ipc::ProfilesActivateOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    let (spec, hash) = state.profiles_activate(&id, &base_hash).map_err(ipc_err)?;
    Ok(supertask_core::ipc::ProfilesActivateOutput {
        spec: serde_yaml::to_value(&spec)
            .map_err(|e| err(ErrorCode::Protocol, format!("序列化 spec 失败: {e}")))?,
        hash,
        active: id,
    })
}

/// `runtime.build`（长操作）：本地服务 package + artifact 选择，不启动（§11）；
/// 1.3 compose 服务路由到 `compose build <service>`（1.3 规格 §6.1，Engine 内置 hub）。
#[tauri::command(rename = "runtime.build")]
pub fn runtime_build(
    state: EngineState<'_>,
    hub: HubState<'_>,
    exiting: State<'_, Exiting>,
    workspace_id: String,
    id: String,
) -> Result<OperationOut, IpcError> {
    ensure_not_exiting(&exiting)?;
    require_current_workspace(&state, &workspace_id)?;
    let is_compose = state
        .spec()
        .ok()
        .and_then(|spec| spec.services.get(&id).map(|s| s.kind == "compose"))
        .unwrap_or(false);
    if is_compose {
        let operation_id = state.build_compose(&id).map_err(ipc_err)?;
        return Ok(OperationOut { operation_id });
    }
    let engine = state.inner().clone();
    let op_id = hub.spawn("runtime.build", move |ctx| {
        ctx.report(None, format!("正在构建 {id}（package + artifact 解析）"));
        let artifact = engine.build_jar(&id)?;
        Ok(json_into(serde_json::json!({
            "id": id,
            "artifact": artifact.to_string_lossy(),
        }))?)
    });
    Ok(OperationOut {
        operation_id: op_id,
    })
}

// ---------------------------------------------------------------------------
// Event bridge: drain Engine events -> Tauri events
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
struct RuntimeEventPayload {
    protocol: u32,
    event: &'static str,
    workspace_id: String,
    ts_ms: u64,
    payload: RuntimePayloadInner,
}

#[derive(Serialize, Clone)]
struct RuntimePayloadInner {
    reason: &'static str,
    services: IndexMap<String, supertask_core::ServiceRuntimeView>,
    script: Option<supertask_core::ScriptRuntimeView>,
    /// 1.6：网关托管状态（未配置时为 None）
    #[serde(skip_serializing_if = "Option::is_none")]
    gateway: Option<supertask_core::GatewayRuntimeView>,
}

#[derive(Serialize, Clone)]
struct LogsEventPayload {
    protocol: u32,
    event: &'static str,
    workspace_id: String,
    ts_ms: u64,
    payload: LogsPayloadInner,
}

#[derive(Serialize, Clone)]
struct LogsPayloadInner {
    items: Vec<supertask_core::log::LogLine>,
}

/// `st.operation` 信封：workspace_id 恒为 null，负载为 hub 的 OperationEvent。
#[derive(Serialize, Clone)]
struct OperationEventPayload {
    protocol: u32,
    event: &'static str,
    workspace_id: Option<String>,
    ts_ms: u64,
    payload: supertask_core::operation::OperationEvent,
}

/// 刷新托盘状态（v1.1 规格 §8.1/§8.2）：tooltip 按当前工作区总体状态更新；
/// 「打开当前工作区 / 启动全部」随有无工作区启/禁。
/// 事件桥传收到的快照；命令侧传 engine 最新快照（无工作区时为空表）。
pub(crate) fn update_tray(
    app: &AppHandle,
    workspace: Option<&str>,
    services: &IndexMap<String, supertask_core::ServiceRuntimeView>,
) {
    let tooltip = if workspace.is_none() {
        "SuperTask".to_string()
    } else if services
        .values()
        .any(|s| matches!(s.state, RtState::Unhealthy))
    {
        "SuperTask — 存在异常".to_string()
    } else if services.values().any(|s| {
        matches!(
            s.state,
            RtState::Starting | RtState::Running | RtState::Stopping
        )
    }) {
        "SuperTask — 运行中".to_string()
    } else {
        "SuperTask — 已停止".to_string()
    };
    if let Some(tray) = app.tray_by_id(TRAY_ID) {
        let _ = tray.set_tooltip(Some(&tooltip));
    }
    if let Some(items) = app.try_state::<TrayItems>() {
        let enabled = workspace.is_some();
        let _ = items.open_workspace.set_enabled(enabled);
        let _ = items.start_all.set_enabled(enabled);
    }
}

/// 命令侧托盘刷新：从 engine 重新读取工作区与快照（open/close/detach/forget 后调用）。
fn refresh_tray_from_engine(app: &AppHandle, state: &EngineState<'_>) {
    let workspace = state.workspace_id().ok();
    let services = state.snapshot().map(|s| s.services).unwrap_or_default();
    update_tray(app, workspace.as_deref(), &services);
}

pub fn spawn_event_bridge(app: AppHandle, engine: Arc<Engine>, hub: HubHandle) {
    std::thread::Builder::new()
        .name("st-event-bridge".into())
        .spawn(move || loop {
            let mut handled = false;
            match engine.try_recv_event() {
                Some(supertask_core::EngineEvent::Runtime(snap)) => {
                    // §8.1：托盘 tooltip / 菜单可用性随运行时事件更新（先借用后 move）
                    update_tray(&app, Some(&snap.workspace_id), &snap.services);
                    let payload = RuntimeEventPayload {
                        protocol: PROTOCOL,
                        event: supertask_core::ipc::event::RUNTIME,
                        workspace_id: snap.workspace_id.clone(),
                        ts_ms: now_ms(),
                        payload: RuntimePayloadInner {
                            reason: "full",
                            services: snap.services,
                            script: snap.script,
                            gateway: snap.gateway,
                        },
                    };
                    let _ = app.emit(supertask_core::ipc::event::RUNTIME, &payload);
                    handled = true;
                }
                Some(supertask_core::EngineEvent::Metrics(payload)) => {
                    let workspace = engine.workspace_id().unwrap_or_default();
                    let ev = supertask_core::ipc::MetricsEvent::new(workspace, now_ms(), payload);
                    let _ = app.emit(supertask_core::ipc::event::METRICS, &ev);
                    handled = true;
                }
                Some(supertask_core::EngineEvent::Logs {
                    workspace_id,
                    items,
                }) => {
                    let payload = LogsEventPayload {
                        protocol: PROTOCOL,
                        event: supertask_core::ipc::event::LOGS,
                        workspace_id,
                        ts_ms: now_ms(),
                        payload: LogsPayloadInner { items },
                    };
                    let _ = app.emit(supertask_core::ipc::event::LOGS, &payload);
                    handled = true;
                }
                None => {}
            }
            match hub.try_recv_event() {
                Some(ev) => {
                    let payload = OperationEventPayload {
                        protocol: PROTOCOL,
                        event: EVENT_OPERATION,
                        workspace_id: None,
                        ts_ms: now_ms(),
                        payload: ev,
                    };
                    let _ = app.emit(EVENT_OPERATION, &payload);
                    handled = true;
                }
                None => {}
            }
            // 1.3 镜像构建走 Engine 内置 OperationHub（与壳层 hub 分开），同样桥到 st.operation
            match engine.operations().try_recv_event() {
                Some(ev) => {
                    let payload = OperationEventPayload {
                        protocol: PROTOCOL,
                        event: EVENT_OPERATION,
                        workspace_id: None,
                        ts_ms: now_ms(),
                        payload: ev,
                    };
                    let _ = app.emit(EVENT_OPERATION, &payload);
                    handled = true;
                }
                None => {}
            }
            if !handled {
                std::thread::sleep(Duration::from_millis(15));
            }
        })
        .expect("spawn event bridge");
}

/// 1.5 §8：导出当前工作区为离线迁移包（zip）。只读操作，不额外取锁。
#[tauri::command(rename = "workspace.exportPackage")]
pub fn workspace_export_package(
    state: EngineState<'_>,
    workspace_id: String,
    dest_path: String,
    with_secrets: bool,
) -> Result<supertask_core::ipc::ExportPackageOut, IpcError> {
    let current = state.workspace_id().map_err(ipc_err)?;
    if workspace_id != current {
        return Err(err(
            ErrorCode::NoWorkspace,
            format!("workspace_id 不匹配当前工作区: {workspace_id}"),
        ));
    }
    let root = std::path::PathBuf::from(&current);
    let out =
        supertask_core::pkg::export_package(&root, std::path::Path::new(&dest_path), with_secrets)
            .map_err(ipc_err)?;
    Ok(supertask_core::ipc::ExportPackageOut {
        path: out.path.display().to_string(),
        entries: out
            .entries
            .iter()
            .map(|e| supertask_core::ipc::PkgEntryView {
                path: e.path.clone(),
                bytes: e.bytes,
            })
            .collect(),
        warnings: out.warnings,
    })
}

/// 1.5 §8：导入导出包（只落盘，不打开不启动）。dest_dir 缺省 cwd。
#[tauri::command(rename = "workspace.importPackage")]
pub fn workspace_import_package(
    pkg_path: String,
    dest_dir: Option<String>,
) -> Result<supertask_core::ipc::ImportPackageOut, IpcError> {
    let dest = match dest_dir {
        Some(d) => std::path::PathBuf::from(d),
        None => std::env::current_dir()
            .map_err(|e| err(ErrorCode::NoWorkspace, format!("无法读取 cwd: {e}")))?,
    };
    let out = supertask_core::pkg::import_package(std::path::Path::new(&pkg_path), &dest)
        .map_err(ipc_err)?;
    Ok(supertask_core::ipc::ImportPackageOut {
        root: out.root.display().to_string(),
        warnings: out.warnings,
    })
}

// ---------------------------------------------------------------------------
// 1.6：网关（core 侧实现；这里只做 IPC 适配。gateway.trust 修改系统信任库，
// UI 层强制确认对话框在前端，本层照常暴露——本地单用户、无网络面）
// ---------------------------------------------------------------------------

#[tauri::command(rename = "gateway.status")]
pub fn gateway_status(
    state: EngineState<'_>,
    workspace_id: String,
) -> Result<supertask_core::ipc::GatewayStatusOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_status().map_err(ipc_err)
}

#[tauri::command(rename = "gateway.preview")]
pub fn gateway_preview(
    state: EngineState<'_>,
    workspace_id: String,
    gateway: Option<supertask_core::spec::GatewayConf>,
) -> Result<supertask_core::ipc::GatewayPreviewOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_preview(gateway).map_err(ipc_err)
}

#[tauri::command(rename = "gateway.validate")]
pub fn gateway_validate(
    state: EngineState<'_>,
    workspace_id: String,
    gateway: Option<supertask_core::spec::GatewayConf>,
) -> Result<supertask_core::ipc::GatewayValidateOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_validate(gateway).map_err(ipc_err)
}

#[tauri::command(rename = "gateway.apply")]
pub fn gateway_apply(
    state: EngineState<'_>,
    workspace_id: String,
    gateway: supertask_core::spec::GatewayConf,
    base_hash: String,
) -> Result<supertask_core::ipc::GatewayApplyOutput, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_apply(gateway, &base_hash).map_err(ipc_err)
}

#[tauri::command(rename = "gateway.start")]
pub fn gateway_start(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    workspace_id: String,
) -> Result<Accepted, IpcError> {
    ensure_not_exiting(&exiting)?;
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_start().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "gateway.stop")]
pub fn gateway_stop(state: EngineState<'_>, workspace_id: String) -> Result<Accepted, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_stop().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "gateway.restart")]
pub fn gateway_restart(
    state: EngineState<'_>,
    exiting: State<'_, Exiting>,
    workspace_id: String,
) -> Result<Accepted, IpcError> {
    ensure_not_exiting(&exiting)?;
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_restart().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

#[tauri::command(rename = "gateway.trust")]
pub fn gateway_trust(state: EngineState<'_>, workspace_id: String) -> Result<Accepted, IpcError> {
    require_current_workspace(&state, &workspace_id)?;
    state.gateway_trust().map_err(ipc_err)?;
    Ok(Accepted {
        accepted: true,
        order: None,
    })
}

// ---------------------------------------------------------------------------
// 2.0 云（v2.0 规格 §11）：八条薄适配；业务在 shell cloud.rs + core cloud/
// ---------------------------------------------------------------------------

#[tauri::command(rename = "cloud.login")]
pub fn cloud_login(
    state: State<'_, crate::cloud::CloudHandle>,
    email: String,
    password: String,
) -> Result<crate::cloud::CloudLoginOut, IpcError> {
    let tokens = crate::cloud::cloud_login(&state, &email, &password).map_err(ipc_err)?;
    Ok(crate::cloud::CloudLoginOut {
        account_id: tokens.account_id,
        email: tokens.email,
        expires_in_secs: tokens.expires_in_secs,
    })
}

#[tauri::command(rename = "cloud.logout")]
pub fn cloud_logout() -> Result<crate::cloud::CloudOkOut, IpcError> {
    crate::cloud::cloud_logout().map_err(ipc_err)?;
    Ok(crate::cloud::CloudOkOut { ok: true })
}

#[tauri::command(rename = "cloud.status")]
pub fn cloud_status(
    state: State<'_, crate::cloud::CloudHandle>,
) -> Result<crate::cloud::CloudStatusOut, IpcError> {
    crate::cloud::cloud_status(&state).map_err(ipc_err)
}

#[tauri::command(rename = "cloud.sync")]
pub fn cloud_sync(
    state: State<'_, crate::cloud::CloudHandle>,
) -> Result<crate::cloud::SyncOut, IpcError> {
    crate::cloud::cloud_sync(&state).map_err(ipc_err)
}

#[tauri::command(rename = "cloud.resolve")]
pub fn cloud_resolve(
    state: State<'_, crate::cloud::CloudHandle>,
    entity_id: String,
    choice: String,
) -> Result<crate::cloud::SyncOut, IpcError> {
    crate::cloud::cloud_resolve(&state, &entity_id, &choice).map_err(ipc_err)
}

#[tauri::command(rename = "cloud.migrate.plan")]
pub fn cloud_migrate_plan(
    state: State<'_, crate::cloud::CloudHandle>,
) -> Result<supertask_core::cloud::migrate::RestorePlan, IpcError> {
    crate::cloud::cloud_migrate_plan(&state).map_err(ipc_err)
}

#[tauri::command(rename = "cloud.migrate.apply")]
pub fn cloud_migrate_apply(
    state: State<'_, crate::cloud::CloudHandle>,
    workspaces: Vec<crate::cloud::MigrateWorkspace>,
    include_templates: Option<bool>,
    include_settings: Option<bool>,
) -> Result<crate::cloud::SyncOut, IpcError> {
    crate::cloud::cloud_migrate_apply(&state, workspaces, include_templates, include_settings)
        .map_err(ipc_err)
}

#[tauri::command(rename = "cloud.endpoint.set")]
pub fn cloud_endpoint_set(
    state: State<'_, crate::cloud::CloudHandle>,
    appdata: AppDataRef<'_>,
    endpoint: String,
) -> Result<crate::cloud::CloudEndpointOut, IpcError> {
    crate::cloud::cloud_endpoint_set(&state, appdata.inner(), &endpoint).map_err(ipc_err)
}

#[tauri::command(rename = "cloud.telemetry.set")]
pub fn cloud_telemetry_set(
    state: State<'_, crate::cloud::CloudHandle>,
    appdata: AppDataRef<'_>,
    enabled: bool,
) -> Result<crate::cloud::CloudTelemetryOut, IpcError> {
    crate::cloud::cloud_telemetry_set(&state, appdata.inner(), enabled).map_err(ipc_err)
}

// ---------------------------------------------------------------------------
// 2.1 AI（v2.1 规格 §5 + 截图对齐升级）：薄适配；业务在 shell ai.rs + core ai/
// ---------------------------------------------------------------------------

/// `ai.config.save`：新建/更新命名配置；api_key 写入 secrets 后端（None 不动，Some("") 清除），不回显。
#[tauri::command(rename = "ai.config.save")]
pub fn ai_config_save(
    appdata: AppDataRef<'_>,
    input: crate::ai::AiConfigSaveIn,
) -> Result<crate::ai::AiConfigOut, IpcError> {
    crate::ai::ai_config_save(appdata.inner(), input).map_err(ipc_err)
}

#[tauri::command(rename = "ai.config.delete")]
pub fn ai_config_delete(appdata: AppDataRef<'_>, id: String) -> Result<(), IpcError> {
    crate::ai::ai_config_delete(appdata.inner(), &id).map_err(ipc_err)
}

#[tauri::command(rename = "ai.config.default")]
pub fn ai_config_default(appdata: AppDataRef<'_>, id: String) -> Result<(), IpcError> {
    crate::ai::ai_config_default(appdata.inner(), &id).map_err(ipc_err)
}

/// `ai.instructions.save`：全局自定义指令（trim；空串清除；≤8000 字符）。
#[tauri::command(rename = "ai.instructions.save")]
pub fn ai_instructions_save(appdata: AppDataRef<'_>, text: String) -> Result<String, IpcError> {
    crate::ai::ai_instructions_save(appdata.inner(), &text).map_err(ipc_err)
}

#[tauri::command(rename = "ai.template.save")]
pub fn ai_template_save(
    appdata: AppDataRef<'_>,
    input: crate::ai::AiTemplateSaveIn,
) -> Result<crate::ai::AiTemplateOut, IpcError> {
    crate::ai::ai_template_save(appdata.inner(), input).map_err(ipc_err)
}

#[tauri::command(rename = "ai.template.delete")]
pub fn ai_template_delete(appdata: AppDataRef<'_>, id: String) -> Result<(), IpcError> {
    crate::ai::ai_template_delete(appdata.inner(), &id).map_err(ipc_err)
}

/// `ai.status`：配置列表/模板/全局指令摘要 + 当日用量；key 只回布尔，不回明文。
#[tauri::command(rename = "ai.status")]
pub fn ai_status(appdata: AppDataRef<'_>) -> Result<crate::ai::AiStatusOut, IpcError> {
    crate::ai::ai_status(appdata.inner()).map_err(ipc_err)
}

/// `ai.models`：OpenAI 兼容端点模型发现（GET /models）。
#[tauri::command(rename = "ai.models")]
pub async fn ai_models(
    appdata: AppDataRef<'_>,
    config_id: Option<String>,
) -> Result<Vec<String>, IpcError> {
    let appdata = appdata.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::ai::ai_models(&appdata, config_id.as_deref())
    })
    .await
    .map_err(|e| err(ErrorCode::Spawn, format!("AI models 后台任务异常: {e}")))?
    .map_err(ipc_err)
}

/// `ai.cli.probe`：本机 CLI 可执行文件探测（`--version`，不保存配置）。
#[tauri::command(rename = "ai.cli.probe")]
pub async fn ai_cli_probe(
    provider: String,
    cli_path: Option<String>,
    cli_env: Option<std::collections::BTreeMap<String, String>>,
) -> Result<supertask_core::ai::cli_agent::CliProbeOut, IpcError> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::ai::ai_cli_probe(&provider, cli_path.as_deref(), &cli_env.unwrap_or_default())
    })
    .await
    .map_err(|e| err(ErrorCode::Spawn, format!("AI CLI probe 后台任务异常: {e}")))?
    .map_err(ipc_err)
}

/// `ai.complete`：仅用户显式触发；task ∈ explain_logs | config_suggest | enrich_draft | test_connection。
#[tauri::command(rename = "ai.complete")]
pub async fn ai_complete(
    app: tauri::AppHandle,
    state: EngineState<'_>,
    appdata: AppDataRef<'_>,
    task: String,
    payload: serde_json::Value,
    config_id: Option<String>,
    request_id: Option<String>,
) -> Result<supertask_core::ai::AiCompleteOut, IpcError> {
    let engine = state.inner().clone();
    let appdata = appdata.inner().clone();
    let stream_emit = request_id
        .filter(|id| !id.is_empty())
        .map(|id| std::sync::Arc::new((app.clone(), id)));
    tauri::async_runtime::spawn_blocking(move || {
        crate::ai::ai_complete(
            &engine,
            &appdata,
            &task,
            &payload,
            config_id.as_deref(),
            stream_emit,
        )
    })
    .await
    .map_err(|e| err(ErrorCode::Spawn, format!("AI complete 后台任务异常: {e}")))?
    .map_err(ipc_err)
}
