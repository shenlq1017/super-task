//! Thin Tauri IPC. Business lives in `supertask-core`.

// 中文版 MSVC link.exe 会在 cdylib 链接时向 stdout 输出「正在创建库 …」，被
// linker_messages lint 误报为 warning；本 crate 链接器输出为已知噪音，允许之。
#![allow(linker_messages)]

use std::path::Path;
use std::sync::Arc;

use serde::Serialize;
use supertask_core::appdata::AppData;
use supertask_core::error::ErrorCode;
use supertask_core::features::{features, Feature};
use supertask_core::ipc::{IpcError, PROTOCOL};
use supertask_core::probe::{probe_toolchain, ToolchainProbe};
use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager, WindowEvent};

pub mod ai;
pub mod cloud;
mod commands;
mod state;
pub mod term;

use state::{AppDataHandle, Exiting, TrayItems};

use commands::spawn_event_bridge;

#[derive(Debug, Serialize)]
pub struct FeatureView {
    pub id: String,
    pub path: String,
    pub status: String,
    pub since: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PrefsView {
    pub theme: String,
    pub restore_last: bool,
    pub close_to_tray: bool,
    pub start_on_login: bool,
    pub update_check: bool,
}

impl PrefsView {
    fn from_appdata(d: &AppData) -> Self {
        Self {
            theme: d.theme.clone(),
            restore_last: d.restore_last,
            close_to_tray: d.close_to_tray,
            start_on_login: d.start_on_login,
            update_check: d.update_check,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct AppLoadOut {
    pub protocol: u32,
    pub prefs: PrefsView,
    pub recents: Vec<String>,
    /// Additive：富最近条目（展示名 + last_opened_ms）；旧客户端只用 `recents`。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_entries: Vec<supertask_core::appdata::RecentEntry>,
    /// Additive：app.json 中的 lastWorkspace（与 prefs.restoreLast 配合）。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_workspace: Option<String>,
    /// 已知失效的最近工作区路径（目录已不存在），供前端标记。
    pub stale: Vec<String>,
    pub probe: ToolchainProbe,
}

fn protocol_err() -> IpcError {
    IpcError {
        protocol: PROTOCOL,
        code: ErrorCode::Protocol,
        message: "protocol 不匹配，请升级 SuperTask".into(),
        retryable: false,
        details: None,
    }
}

fn feature_view(f: &Feature) -> FeatureView {
    FeatureView {
        id: f.id.into(),
        path: f.path.into(),
        status: match f.status {
            supertask_core::features::FeatureStatus::Live => "live".into(),
            supertask_core::features::FeatureStatus::Preview => "preview".into(),
            supertask_core::features::FeatureStatus::Soon => "soon".into(),
        },
        since: f.since.into(),
    }
}

#[tauri::command(rename = "session.hello")]
fn session_hello(client: String, protocol: u32) -> Result<HelloOut, IpcError> {
    let _ = client;
    if protocol != PROTOCOL {
        return Err(protocol_err());
    }
    Ok(HelloOut {
        protocol: PROTOCOL,
        engine: "supertask-core".into(),
        engine_version: env!("CARGO_PKG_VERSION").into(),
        product_version: env!("CARGO_PKG_VERSION").into(),
        os: std::env::consts::OS.into(),
        features: features().iter().map(feature_view).collect(),
    })
}

#[tauri::command(rename = "app.load")]
fn app_load(appdata: tauri::State<'_, AppDataHandle>) -> Result<AppLoadOut, IpcError> {
    // 失效检测：目录已不存在的 recent 标记 stale；本次新标记的回写一次磁盘。
    let (prefs, recents, recent_entries, last_workspace, stale, changed) = {
        let mut data = appdata.lock().expect("appdata lock");
        let mut changed = false;
        for p in data.recents.clone() {
            if !Path::new(&p).is_dir() {
                let before = data.stale.len();
                data.mark_stale(&p);
                changed |= data.stale.len() != before;
            }
        }
        (
            PrefsView::from_appdata(&data),
            data.recents.clone(),
            data.recent_entries(),
            data.last_workspace.clone(),
            data.stale.clone(),
            changed,
        )
    };
    if changed {
        // 回写失败不阻塞 load（下次启动会再次检测）
        let _ = state::save_appdata(&appdata);
    }
    Ok(AppLoadOut {
        protocol: PROTOCOL,
        prefs,
        recents,
        recent_entries,
        last_workspace,
        stale,
        probe: probe_toolchain(),
    })
}

#[derive(Debug, Serialize)]
pub struct HelloOut {
    pub protocol: u32,
    pub engine: String,
    pub engine_version: String,
    pub product_version: String,
    pub os: String,
    pub features: Vec<FeatureView>,
}

/// 托盘 tooltip 基础文案（无工作区）。
const TRAY_TOOLTIP_BASE: &str = "SuperTask";

/// 退出流程（v1.1 规格 §8.3）：标记退出 → Engine close（停全部 + 等脚本退出 + 释放 Job）
/// → 清托盘与窗口 → 退出进程。幂等：重复触发直接返回。
/// Engine 失败时不假报成功：eprintln 记录并以非 0 码退出。
pub(crate) fn request_exit(app: &AppHandle) {
    if state::mark_exiting(&app.state::<Exiting>()) {
        return;
    }
    let engine = app.state::<Arc<supertask_core::Engine>>();
    // 终端会话先清场（ConPTY 关闭即整树终止），再停工作区
    {
        let term = app.state::<term::TermHandle>();
        term.0.close_all();
    }
    let result = engine.close();
    // §8.3 第 4 步：关托盘与主窗口（顺带清理 resources_table）
    app.cleanup_before_exit();
    match result {
        Ok(()) => app.exit(0),
        Err(e) => {
            eprintln!("[supertask] 退出时清理工作区失败: {e}");
            app.exit(1);
        }
    }
}

/// 构建托盘（id "main"）：内置应用图标 + 中文菜单；无工作区时禁用工作区项。
fn build_tray(app: &AppHandle) -> tauri::Result<TrayItems> {
    let show = MenuItem::with_id(app, "show", "显示 SuperTask", true, None::<&str>)?;
    let open_workspace =
        MenuItem::with_id(app, "open_workspace", "打开当前工作区", false, None::<&str>)?;
    let start_all = MenuItem::with_id(app, "start_all", "启动全部", false, None::<&str>)?;
    let stop_all = MenuItem::with_id(app, "stop_all", "停止全部", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出 SuperTask", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &open_workspace, &start_all, &stop_all, &quit])?;
    TrayIconBuilder::with_id(commands::TRAY_ID)
        .icon(app.default_window_icon().expect("app icon").clone())
        .tooltip(TRAY_TOOLTIP_BASE)
        .menu(&menu)
        .on_menu_event(|app, event| match event.id().as_ref() {
            "show" => {
                if let Some(w) = app.get_webview_window("main") {
                    let _ = w.show();
                    let _ = w.set_focus();
                }
            }
            // 与 workspace.openExplorer 同逻辑（commands::open_in_explorer）
            "open_workspace" => {
                let engine = app.state::<Arc<supertask_core::Engine>>();
                match engine.workspace_id() {
                    Ok(id) => {
                        if let Err(e) = commands::open_in_explorer(&id, None) {
                            eprintln!("[supertask] 托盘打开工作区失败: {e:?}");
                        }
                    }
                    Err(e) => eprintln!("[supertask] 托盘打开工作区失败: {e}"),
                }
            }
            "start_all" => {
                let engine = app.state::<Arc<supertask_core::Engine>>();
                if let Err(e) = engine.start_all() {
                    eprintln!("[supertask] 托盘启动全部失败: {e}");
                }
            }
            "stop_all" => {
                let engine = app.state::<Arc<supertask_core::Engine>>();
                if let Err(e) = engine.stop_all() {
                    eprintln!("[supertask] 托盘停止全部失败: {e}");
                }
            }
            "quit" => request_exit(app),
            _ => {}
        })
        .build(app)?;
    Ok(TrayItems {
        open_workspace,
        start_all,
    })
}

/// 启动时对账开机启动：偏好开启但系统未注册 → 补注册一次（尽力而为，失败不阻塞启动）。
fn reconcile_autostart(app: &AppHandle) {
    use tauri_plugin_autostart::ManagerExt as _;
    let want = {
        let appdata = app.state::<AppDataHandle>();
        let data = appdata.lock().expect("appdata lock");
        data.start_on_login
    };
    if !want {
        return;
    }
    let mgr = app.autolaunch();
    match mgr.is_enabled() {
        Ok(true) => {}
        Ok(false) => {
            if let Err(e) = mgr.enable() {
                eprintln!("[supertask] 开机启动注册失败: {e}");
            }
        }
        Err(e) => eprintln!("[supertask] 检查开机启动状态失败: {e}"),
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let engine = Arc::new(supertask_core::Engine::new());
    let hub = state::new_hub();
    let appdata = state::init_appdata();
    let pending_update = state::new_pending_update();
    // 运行页终端（ipc.md §10.15）：PTY 会话托管（桥线程在 setup 中启动）
    let term_mgr = Arc::new(supertask_core::term::PtyManager::new());
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        // 开机启动：不带启动参数——只启动 SuperTask 本身，不执行 runtime.startAll（§8.4）
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_updater::Builder::new().build())
        // 1.7 §8.2：服务崩溃系统通知（失焦时）
        .plugin(tauri_plugin_notification::init())
        .manage(engine.clone())
        .manage(hub.clone())
        .manage(appdata.clone())
        .manage(term::TermHandle(term_mgr.clone()))
        .manage({
            let data = appdata.lock().expect("appdata lock");
            cloud::CloudHandle::new(&data)
        })
        .manage(state::new_exiting())
        .manage(pending_update)
        // 关闭到托盘（§8.1/§8.3）：CloseRequested 一律 prevent_close；
        // closeToTray=true → 隐藏主窗口；否则走退出流程。
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let app = window.app_handle();
                let close_to_tray = {
                    let appdata = app.state::<AppDataHandle>();
                    let data = appdata.lock().expect("appdata lock");
                    data.close_to_tray
                };
                if close_to_tray {
                    let _ = window.hide();
                } else {
                    request_exit(&app);
                }
            }
        })
        .setup(move |app| {
            spawn_event_bridge(app.handle().clone(), engine, hub);
            term::spawn_term_bridge(app.handle().clone(), term_mgr);

            let tray_items = build_tray(app.handle())?;
            app.manage(tray_items);

            reconcile_autostart(app.handle());

            // 按偏好启动时后台检查更新一次（§9.1：失败不阻塞、不提示打扰）
            let update_check = {
                let appdata = app.state::<AppDataHandle>();
                let data = appdata.lock().expect("appdata lock");
                data.update_check
            };
            if update_check {
                commands::spawn_update_check(app.handle().clone(), true);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            session_hello,
            app_load,
            commands::workspace_add,
            commands::workspace_open,
            commands::workspace_close,
            commands::workspace_detach,
            commands::workspace_forget,
            commands::workspace_scan_draft,
            commands::workspace_open_explorer,
            commands::workspace_init,
            commands::cloud_login,
            commands::cloud_logout,
            commands::cloud_status,
            commands::cloud_sync,
            commands::cloud_resolve,
            commands::cloud_migrate_plan,
            commands::cloud_migrate_apply,
            commands::cloud_telemetry_set,
            commands::cloud_endpoint_set,
            commands::workspace_open_ide,
            commands::workspace_scan_preview,
            commands::workspace_scan_apply,
            commands::import_taskfile_preview,
            commands::import_taskfile_apply,
            commands::import_readme_preview,
            commands::import_readme_apply,
            commands::workspace_adopt_preview,
            commands::workspace_adopt_apply,
            commands::workspace_needs_resolve,
            commands::system_discover,
            commands::system_kill_process,
            commands::yaml_get,
            commands::yaml_save_text,
            commands::yaml_save_form,
            commands::runtime_snapshot,
            commands::runtime_start_one,
            commands::runtime_start_all,
            commands::runtime_stop_one,
            commands::runtime_stop_all,
            commands::runtime_restart_one,
            commands::script_run,
            commands::script_cancel,
            commands::toolchain_probe,
            commands::app_save_prefs,
            commands::app_import_recents,
            commands::logs_subscribe,
            commands::logs_unsubscribe,
            commands::logs_snapshot,
            commands::logs_clear_view,
            commands::templates_list,
            commands::templates_create,
            commands::templates_preview,
            commands::templates_import,
            commands::templates_export,
            commands::workspace_export_package,
            commands::workspace_import_package,
            // 方向六：数据快照（ipc.md §10.18）
            commands::workspace_data_list,
            commands::workspace_data_snapshot_create,
            commands::workspace_data_restore_preview,
            commands::workspace_data_restore,
            commands::workspace_data_snapshot_delete,
            commands::git_clone,
            commands::git_status,
            commands::git_pull,
            commands::system_metrics,
            commands::system_info,
            commands::docker_probe,
            commands::docker_ps,
            commands::docker_images,
            commands::docker_build,
            commands::docker_build_cancel,
            // 2.1 AI
            commands::ai_config_save,
            commands::ai_config_delete,
            commands::ai_config_default,
            commands::ai_instructions_save,
            commands::ai_template_save,
            commands::ai_template_delete,
            commands::ai_status,
            commands::ai_models,
            commands::ai_cli_probe,
            commands::ai_complete,
            commands::toolchain_install,
            commands::toolchain_upgrade,
            commands::toolchain_versions,
            commands::ports_inspect,
            commands::env_effective,
            commands::spring_inspect,
            commands::ports_suggest,
            commands::ports_assign,
            commands::secrets_status,
            commands::secrets_set,
            commands::secrets_delete,
            commands::secrets_validate,
            commands::logs_search,
            commands::logs_export,
            commands::logs_retention_run,
            commands::metrics_snapshot,
            commands::metrics_subscribe,
            commands::metrics_unsubscribe,
            commands::profiles_list,
            commands::profiles_activate,
            commands::runtime_build,
            commands::app_update_check,
            commands::app_update_install,
            commands::app_write_text_file,
            // 1.6 网关
            commands::gateway_status,
            commands::gateway_preview,
            commands::gateway_validate,
            commands::gateway_apply,
            commands::gateway_start,
            commands::gateway_stop,
            commands::gateway_restart,
            commands::gateway_trust,
            // 运行页终端（ipc.md §10.15）
            term::term_open,
            term::term_write,
            term::term_resize,
            term::term_close,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
