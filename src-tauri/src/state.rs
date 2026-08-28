//! 壳层托管状态：长操作 hub 与应用数据（`%APPDATA%/SuperTask/app.json`）。
//! 数据模型与读写语义都在 supertask-core；这里只做路径定位与 Arc 托管。

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use supertask_core::appdata::{self, AppData};
use supertask_core::error::{Error, ErrorCode};
use supertask_core::operation::OperationHub;

/// 长操作 hub 托管句柄。
pub type HubHandle = Arc<OperationHub>;

/// 应用数据托管句柄；磁盘真源 `%APPDATA%/SuperTask/app.json`。
pub type AppDataHandle = Arc<Mutex<AppData>>;

/// 全局退出标记：置位后拒绝新的启动、模板与 Git 操作（v1.1 规格 §8.3 退出顺序）。
pub type Exiting = Arc<AtomicBool>;

/// 最近一次检查发现的可用更新；`app.update.install` 按版本号匹配后消费。
/// `tauri_plugin_updater::Update` 为 Clone + Send + Sync（docs.rs），可安全托管。
pub type PendingUpdate = Arc<Mutex<Option<tauri_plugin_updater::Update>>>;

/// 托盘菜单项句柄：无当前工作区时禁用「打开当前工作区 / 启动全部」（§8.2）。
/// `MenuItem<R>` Send + Sync，可托管；句柄与托盘菜单内的项共享同一实例。
pub struct TrayItems {
    pub open_workspace: tauri::menu::MenuItem<tauri::Wry>,
    pub start_all: tauri::menu::MenuItem<tauri::Wry>,
}

pub fn new_hub() -> HubHandle {
    Arc::new(OperationHub::new())
}

pub fn new_exiting() -> Exiting {
    Arc::new(AtomicBool::new(false))
}

pub fn new_pending_update() -> PendingUpdate {
    Arc::new(Mutex::new(None))
}

/// 退出标记读取（SeqCst：置位必须对随后所有命令可见）。
pub fn is_exiting(flag: &Exiting) -> bool {
    flag.load(Ordering::SeqCst)
}

/// 退出标记置位；返回置位前的值（true = 已在退出流程，调用方应直接返回）。
pub fn mark_exiting(flag: &Exiting) -> bool {
    flag.swap(true, Ordering::SeqCst)
}

/// 启动时从磁盘加载 app data；APPDATA 缺失或文件损坏时回退 Default。
pub fn init_appdata() -> AppDataHandle {
    let data = appdata_path().map_or_else(|_| AppData::default(), |p| appdata::load_at(&p));
    Arc::new(Mutex::new(data))
}

/// `%APPDATA%/SuperTask/app.json`；APPDATA 环境变量缺失时报 `CwdMissing`。
fn appdata_path() -> Result<PathBuf, Error> {
    let base = std::env::var("APPDATA")
        .map_err(|_| Error::new(ErrorCode::CwdMissing, "无法定位应用数据目录"))?;
    Ok(PathBuf::from(base).join("SuperTask").join("app.json"))
}

/// 把托管 app data 原子写回磁盘。
pub fn save_appdata(handle: &AppDataHandle) -> Result<(), Error> {
    let path = appdata_path()?;
    let data = handle.lock().expect("appdata lock");
    appdata::save_at(&path, &data)
}
