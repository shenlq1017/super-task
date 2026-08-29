//! 2.0 §14：`supertask cloud status|sync|logout`。与桌面共享 appdata 会话；
//! 未登录 → CLOUD_NOT_LOGGED_IN + 人话提示「请在桌面端登录」。

use supertask_core::cloud::http::HttpCloudProvider;
use supertask_core::cloud::session;
use supertask_core::cloud::CloudProvider;
use supertask_core::error::{ErrorCode, Result};

use crate::output;

pub fn run_cloud(json: bool, cmd: &crate::cli::CloudCmd) -> Result<i32> {
    match cmd {
        crate::cli::CloudCmd::Status => run_status(json),
        crate::cli::CloudCmd::Sync => run_sync(json),
        crate::cli::CloudCmd::Logout => run_logout(json),
    }
}

fn provider() -> HttpCloudProvider {
    // 端点：appdata `cloud_endpoint`（app.json），缺省内置占位端点
    let app =
        supertask_core::appdata::load_at(&supertask_core::appdata::appdata_dir().join("app.json"));
    HttpCloudProvider::new(
        app.cloud_endpoint
            .clone()
            .unwrap_or_else(|| supertask_core::cloud::http::DEFAULT_ENDPOINT.into()),
    )
}

fn run_status(json: bool) -> Result<i32> {
    let tokens = session::load_session()?;
    let p = provider();
    let quota = p.quota(&tokens.access_token).ok();
    let state = supertask_core::cloud::sync::load_state();
    let conflicts = state.conflicts.len();
    if json {
        output::ok(
            true,
            serde_json::json!({
                "logged_in": true, "email": tokens.email, "device": session::device_id(),
                "last_synced_ms": state.last_synced_ms, "conflicts": conflicts,
                "quota": quota,
            }),
        );
    } else {
        println!("已登录: {}", tokens.email);
        println!("设备: {}", session::device_id());
        println!("冲突: {conflicts}");
        if let Some(q) = quota {
            println!(
                "配额: {}/{} 实体，{}/{} 字节",
                q.entities, q.entities_max, q.bytes, q.bytes_max
            );
        }
    }
    Ok(0)
}

fn run_sync(json: bool) -> Result<i32> {
    let tokens = session::load_session().map_err(|e| {
        if e.code() == ErrorCode::CloudNotLoggedIn {
            supertask_core::Error::new(
                ErrorCode::CloudNotLoggedIn,
                "未登录。请在桌面端登录后再使用 cloud sync",
            )
        } else {
            e
        }
    })?;
    let p = provider();
    let mut state = supertask_core::cloud::sync::load_state();
    // CLI 侧仅只读绑定（无 settings/template 写权限设计前先只 push/pull workspace）；
    // 首版：无本地绑定 → 只报告远端与本地状态差异（安全默认，不做落盘）
    let remote = p.list(&tokens.access_token, None)?;
    let tracked = state.entities.len();
    let mut data = serde_json::json!({
        "remote_entities": remote.len(),
        "tracked_entities": tracked,
        "conflicts": state.conflicts.len(),
        "note": "CLI sync 首版为只读预览；落盘同步请在桌面端执行",
    });
    if json {
        output::ok(true, data);
    } else {
        println!("云端实体: {}", remote.len());
        println!("本地跟踪: {tracked}");
        println!("待解决冲突: {}", state.conflicts.len());
        println!("（CLI sync 首版为只读预览；落盘同步请在桌面端执行）");
    }
    Ok(0)
}

fn run_logout(json: bool) -> Result<i32> {
    session::clear_session()?;
    if json {
        output::ok(true, serde_json::json!({ "ok": true }));
    } else {
        println!("已登出（本地数据与同步状态保留）");
    }
    Ok(0)
}
