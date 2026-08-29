//! 工作区所有权锁（1.5 §3.1）：同一工作区同一时刻只有一个存活进程 owner。
//!
//! 协议：`<root>/.supertask/engine.lock`，JSON `{ pid, holder, started_at_ms }`。
//! 获取 = create-new 独占创建；同 pid 重入合法（进程内复用）；他 pid 存活 →
//! `WORKSPACE_LOCKED`（details 带 holder/pid）；持有 pid 已死或内容损坏 → stale，
//! 清理后接管。释放 = 仅当锁内 pid 与当前进程一致时删除。stale 探测是崩溃兜底，
//! 不依赖优雅退出。锁只读探测 pid 存活，绝不向任意 pid 发信号。

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};
use crate::proc::pid_alive;

/// 锁持有者标签（与引擎前端一一对应；serde 输出小写字符串）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LockHolder {
    Desktop,
    Cli,
    Mcp,
}

impl LockHolder {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Desktop => "desktop",
            Self::Cli => "cli",
            Self::Mcp => "mcp",
        }
    }
}

/// 锁文件内容。字段与规格 §3.1 一致，只增不破。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LockInfo {
    pub pid: u32,
    pub holder: LockHolder,
    pub started_at_ms: u64,
}

pub fn lock_path(root: &Path) -> PathBuf {
    root.join(".supertask").join("engine.lock")
}

/// 只读读取锁内容；缺失或损坏返回 None（status/只读工具展示用，不做 stale 清理）。
pub fn query(root: &Path) -> Option<LockInfo> {
    let bytes = fs::read(lock_path(root)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// 获取工作区锁。他 pid 存活 → `WORKSPACE_LOCKED`（details 携带 holder 与 pid）；
/// 同 pid 重入 → 原样返回既有锁。
pub fn acquire(root: &Path, holder: LockHolder) -> Result<LockInfo> {
    let dir = root.join(".supertask");
    fs::create_dir_all(&dir).map_err(|e| {
        Error::new(
            ErrorCode::NoWorkspace,
            format!("无法创建 .supertask 目录: {e}"),
        )
    })?;
    // create_new 竞争窗口极小，失败后重读一轮即可；不引入轮询。
    for _ in 0..2 {
        match existing_lock_state(root)? {
            ExistingState::Reentrant(info) => return Ok(info),
            ExistingState::Contended(err) => return Err(err),
            ExistingState::Vacant => {}
        }
        match try_create(&dir, holder) {
            Ok(info) => return Ok(info),
            // 他人抢先创建：下一轮重读既有锁再判
            Err(LockRace::Contended) => {}
            Err(LockRace::Io(e)) => {
                return Err(Error::new(
                    ErrorCode::NoWorkspace,
                    format!("无法写入工作区锁: {e}"),
                ))
            }
        }
    }
    // 第二轮仍竞争：并发获取极端竞争，按被持有处理
    Err(locked_error(root))
}

enum LockRace {
    Contended,
    Io(std::io::Error),
}

fn try_create(dir: &Path, holder: LockHolder) -> std::result::Result<LockInfo, LockRace> {
    let info = LockInfo {
        pid: std::process::id(),
        holder,
        started_at_ms: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0),
    };
    let mut opts = fs::OpenOptions::new();
    opts.write(true).create_new(true);
    let path = dir.join("engine.lock");
    let res = opts.open(&path);
    match res {
        Ok(mut file) => {
            serde_json::to_writer(&mut file, &info)
                .map_err(|e| LockRace::Io(std::io::Error::other(e)))?;
            Ok(info)
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Err(LockRace::Contended),
        Err(e) => Err(LockRace::Io(e)),
    }
}

enum ExistingState {
    /// 同 pid 重入
    Reentrant(LockInfo),
    /// 他 pid 存活
    Contended(Error),
    /// 无锁或 stale（pid 已死 / JSON 损坏，已清理）
    Vacant,
}

fn existing_lock_state(root: &Path) -> Result<ExistingState> {
    let bytes = match fs::read(lock_path(root)) {
        Ok(bytes) => bytes,
        // 已被其他进程清理/重建：视为无锁
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(ExistingState::Vacant),
        Err(e) => {
            return Err(Error::new(
                ErrorCode::NoWorkspace,
                format!("无法读取工作区锁: {e}"),
            ))
        }
    };
    match serde_json::from_slice::<LockInfo>(&bytes) {
        Ok(info) if info.pid == std::process::id() => Ok(ExistingState::Reentrant(info)),
        Ok(info) if pid_alive(info.pid) => Ok(ExistingState::Contended(locked_error_with(info))),
        // pid 已死（stale）或 JSON 损坏：清理后重建
        _ => {
            let _ = fs::remove_file(lock_path(root));
            Ok(ExistingState::Vacant)
        }
    }
}

fn locked_error(root: &Path) -> Error {
    let info = query(root);
    match info {
        Some(info) => locked_error_with(info),
        None => Error::new(ErrorCode::WorkspaceLocked, "工作区锁被其他进程占用"),
    }
}

fn locked_error_with(info: LockInfo) -> Error {
    Error::new(
        ErrorCode::WorkspaceLocked,
        format!(
            "工作区已被 {} (pid {}) 持有，请先关闭持有进程",
            info.holder.as_str(),
            info.pid
        ),
    )
    .details(serde_yaml::Value::Mapping(holder_details(
        info.holder,
        info.pid,
    )))
}

fn holder_details(holder: LockHolder, pid: u32) -> serde_yaml::Mapping {
    let mut m = serde_yaml::Mapping::new();
    m.insert(
        serde_yaml::Value::String("holder".into()),
        serde_yaml::Value::String(holder.as_str().into()),
    );
    m.insert(
        serde_yaml::Value::String("pid".into()),
        serde_yaml::Value::Number(pid.into()),
    );
    m
}

/// 释放锁：仅当锁内 pid 与当前进程一致才删除（不删别人的锁）；锁不存在视为已释放。
pub fn release(root: &Path) -> Result<()> {
    match query(root) {
        Some(info) if info.pid == std::process::id() => match fs::remove_file(lock_path(root)) {
            Ok(()) => Ok(()),
            // 并发下已被清理：视为已释放
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(Error::new(
                ErrorCode::NoWorkspace,
                format!("无法删除工作区锁: {e}"),
            )),
        },
        // 他人持有或锁损坏：不动文件，stale 由下次 acquire 兜底
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 平台无关的「存活但非本进程」pid：Windows System 固定 pid 4；Unix pid 1 必在。
    fn foreign_alive_pid() -> u32 {
        if cfg!(windows) {
            4
        } else {
            1
        }
    }

    /// 平台无关的「几乎不可能存活」pid（不 spawn 外部进程，遵守测试隔离纪律）。
    fn dead_pid() -> u32 {
        4_000_000
    }

    fn write_lock(root: &Path, pid: u32, holder: LockHolder) {
        let dir = root.join(".supertask");
        fs::create_dir_all(&dir).unwrap();
        let info = LockInfo {
            pid,
            holder,
            started_at_ms: 0,
        };
        fs::write(lock_path(root), serde_json::to_vec(&info).unwrap()).unwrap();
    }

    #[test]
    fn acquire_then_reacquire_same_pid_reenters() {
        let tmp = tempfile_root("acquire_reentry");
        let first = acquire(&tmp, LockHolder::Desktop).unwrap();
        let second = acquire(&tmp, LockHolder::Desktop).unwrap();
        assert_eq!(first, second);
        assert_eq!(first.pid, std::process::id());
        release(&tmp).unwrap();
        assert!(!lock_path(&tmp).exists());
    }

    #[test]
    fn acquire_rejects_alive_foreign_pid_with_details() {
        let tmp = tempfile_root("acquire_foreign");
        write_lock(&tmp, foreign_alive_pid(), LockHolder::Cli);
        let err = acquire(&tmp, LockHolder::Desktop).unwrap_err();
        assert_eq!(err.code(), ErrorCode::WorkspaceLocked);
        let details = match err {
            crate::error::Error::App { details, .. } => details,
        };
        let details = details.expect("details carry holder/pid");
        let map = details.as_mapping().expect("mapping details");
        let holder = map
            .get(&serde_yaml::Value::String("holder".into()))
            .and_then(|v| v.as_str())
            .expect("holder detail");
        let pid = map
            .get(&serde_yaml::Value::String("pid".into()))
            .and_then(|v| v.as_u64())
            .expect("pid detail");
        assert_eq!(holder, "cli");
        assert_eq!(pid as u32, foreign_alive_pid());
        // 清理测试锁文件（不动存活进程）
        let _ = fs::remove_file(lock_path(&tmp));
    }

    #[test]
    fn acquire_takes_over_stale_lock_of_dead_pid() {
        let tmp = tempfile_root("acquire_stale");
        write_lock(&tmp, dead_pid(), LockHolder::Mcp);
        let info = acquire(&tmp, LockHolder::Cli).unwrap();
        assert_eq!(info.pid, std::process::id());
        assert_eq!(info.holder, LockHolder::Cli);
        release(&tmp).unwrap();
    }

    #[test]
    fn acquire_rebuilds_corrupt_lock_file() {
        let tmp = tempfile_root("acquire_corrupt");
        let dir = tmp.join(".supertask");
        fs::create_dir_all(&dir).unwrap();
        fs::write(lock_path(&tmp), b"{not json").unwrap();
        let info = acquire(&tmp, LockHolder::Desktop).unwrap();
        assert_eq!(info.pid, std::process::id());
        // 重建后的文件是合法 JSON
        assert!(query(&tmp).is_some());
        release(&tmp).unwrap();
    }

    #[test]
    fn release_refuses_foreign_lock() {
        let tmp = tempfile_root("release_foreign");
        write_lock(&tmp, foreign_alive_pid(), LockHolder::Cli);
        release(&tmp).unwrap();
        // 存活他人锁不被本进程删除
        assert!(lock_path(&tmp).exists());
        let _ = fs::remove_file(lock_path(&tmp));
    }

    #[test]
    fn lock_info_json_round_trip() {
        let info = LockInfo {
            pid: 42,
            holder: LockHolder::Mcp,
            started_at_ms: 1_700_000_000_000,
        };
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"holder\":\"mcp\""), "{json}");
        let back: LockInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back, info);
    }

    #[test]
    fn pid_alive_self_true_and_implausible_pid_false() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(dead_pid()));
    }

    /// 测试临时根目录（tests/temp 下，按用例名隔离；进程内创建，无外部进程依赖）。
    fn tempfile_root(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("supertask-lock-test-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }
}
