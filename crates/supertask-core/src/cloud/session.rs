//! 会话与 token 存储（v2.0 规格 §5）。
//! - 会话文件：`%APPDATA%/SuperTask/cloud/session.json`；Windows DPAPI 静态加密
//!   （`encrypted: true` + hex payload），Unix / DPAPI 失败回退明文（文档在 cloud.md 标注）；
//! - 401 → refresh 一次 → 重放由 sync 层负责；refresh 失效 → `CLOUD_AUTH_FAILED` 转登出态；
//! - 设备 id = sha256(hostname + 首启时间)（复用 sha2，只用于 `updated_by` 展示与排障）；
//! - CLI 与桌面共享同一会话文件。
//!
//! 密码不落盘、不进日志（本模块只接触 token）。

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

use super::{sha256_hex, LoginTokens};
use crate::error::{Error, ErrorCode, Result};

pub fn session_dir() -> PathBuf {
    #[cfg(test)]
    {
        static TEST_DIR: std::sync::OnceLock<PathBuf> = std::sync::OnceLock::new();
        return TEST_DIR
            .get_or_init(|| {
                std::env::temp_dir().join(format!("st-cloud-session-{}", std::process::id()))
            })
            .clone();
    }
    #[cfg(not(test))]
    crate::appdata::appdata_dir().join("cloud")
}

pub fn session_path() -> PathBuf {
    session_dir().join("session.json")
}

/// 设备元数据路径。设备首启时间与同步状态分开保存，避免 device_id 覆盖 state.json。
pub fn device_meta_path() -> PathBuf {
    session_dir().join("meta").join("device.json")
}

/// 会话容器：`encrypted=true` 时 `payload` 为 DPAPI 密文的 hex；否则 tokens 明文。
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SessionContainer {
    encrypted: bool,
    #[serde(default)]
    payload: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    tokens: Option<LoginTokens>,
}

/// 设备 id：sha256(hostname + 首启时间戳) 前 16 hex；首启时间戳保存在独立 meta/device.json。
/// 进程内 memoize：并发首调（测试全量并发等）只产生一次时间戳——否则两个线程
/// 各自 miss → 各自 now() 落盘并返回不同值。跨进程仍以文件里先落盘者为准。
pub fn device_id() -> String {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE
        .get_or_init(|| {
            let host = std::env::var("COMPUTERNAME")
                .or_else(|_| std::env::var("HOSTNAME"))
                .unwrap_or_else(|_| "unknown-host".into());
            let first_run = first_run_ms(&device_meta_path());
            sha256_hex(format!("{host}{first_run}").as_bytes())[..16].to_string()
        })
        .clone()
}

fn first_run_ms(path: &PathBuf) -> u64 {
    if let Some(ms) = read_first_run(path) {
        return ms;
    }
    // Migrate the old location without touching state.json. Older clients
    // stored this metadata in the sync state file, so preserve the id across
    // the move while keeping future device writes isolated.
    let legacy = session_dir().join("state.json");
    if legacy.as_path() != path {
        if let Some(ms) = read_first_run(&legacy) {
            let _ = fs::create_dir_all(path.parent().unwrap_or(&session_dir()));
            let text = serde_json::json!({ "first_run_ms": ms }).to_string();
            let _ = atomic_write(path, &text);
            return ms;
        }
    }
    let now = now_ms();
    let _ = fs::create_dir_all(path.parent().unwrap_or(&session_dir()));
    let text = serde_json::json!({ "first_run_ms": now }).to_string();
    let _ = atomic_write(path, &text);
    now
}

fn read_first_run(path: &PathBuf) -> Option<u64> {
    let txt = fs::read_to_string(path).ok()?;
    let value = serde_json::from_str::<serde_json::Value>(&txt).ok()?;
    value
        .get("first_run_ms")
        .and_then(serde_json::Value::as_u64)
}

fn atomic_write(path: &std::path::Path, text: &str) -> Result<()> {
    static NEXT_TMP: AtomicU64 = AtomicU64::new(0);
    let n = NEXT_TMP.fetch_add(1, Ordering::Relaxed);
    let mut tmp = path.as_os_str().to_os_string();
    tmp.push(format!(".tmp-{}-{n}", std::process::id()));
    let tmp = PathBuf::from(tmp);
    let mut file = fs::File::create(&tmp)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("临时文件创建失败: {e}")))?;
    file.write_all(text.as_bytes())
        .and_then(|_| file.sync_all())
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("临时文件写入失败: {e}")))?;
    // Windows cannot replace or move a file while this handle is still open.
    // Explicitly close it before ReplaceFileW/MoveFileExW, including the
    // overwrite path used when refreshing an existing session.
    drop(file);
    // A first write has no destination for ReplaceFileW to replace. Rust's
    // rename is atomic for this same-volume move and avoids the Windows API
    // error returned by trying ReplaceFileW/MoveFileExW on a missing target.
    let replace_result = if path.exists() {
        replace_file(&tmp, path)
    } else {
        fs::rename(&tmp, path)
    };
    if let Err(rename_error) = replace_result {
        let _ = fs::remove_file(&tmp);
        return Err(Error::new(
            ErrorCode::Protocol,
            format!("原子替换失败: {rename_error}"),
        ));
    }
    Ok(())
}

#[cfg(not(windows))]
fn replace_file(tmp: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
    fs::rename(tmp, path)
}

#[cfg(windows)]
fn replace_file(tmp: &std::path::Path, path: &std::path::Path) -> std::io::Result<()> {
    use std::os::windows::ffi::OsStrExt;

    // ReplaceFileW performs an atomic replacement when the destination
    // exists; MoveFileExW handles the first write when it does not.
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn ReplaceFileW(
            replaced: *const u16,
            replacement: *const u16,
            backup: *const u16,
            flags: u32,
            exclude: *const std::ffi::c_void,
            reserved: *const std::ffi::c_void,
        ) -> i32;
    }
    let old: Vec<u16> = tmp.as_os_str().encode_wide().chain(Some(0)).collect();
    let new: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let replaced = unsafe {
        ReplaceFileW(
            new.as_ptr(),
            old.as_ptr(),
            std::ptr::null(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if replaced != 0 {
        return Ok(());
    }
    Err(std::io::Error::last_os_error())
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// DPAPI（仅 Windows）；失败 → 回退明文 + `encrypted:false`（spec §5 允许）
// ---------------------------------------------------------------------------

#[cfg(windows)]
fn dpapi_protect(plain: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CryptProtectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: plain.len() as u32,
            pbData: plain.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptProtectData(
            &mut input,
            windows::core::PCWSTR::null(),
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("DPAPI 加密失败: {e}")))?;
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let out = slice.to_vec();
        free_blob(output);
        Ok(out)
    }
}

#[cfg(windows)]
fn dpapi_unprotect(cipher: &[u8]) -> Result<Vec<u8>> {
    use windows::Win32::Security::Cryptography::{
        CryptUnprotectData, CRYPTPROTECT_UI_FORBIDDEN, CRYPT_INTEGER_BLOB,
    };
    unsafe {
        let mut input = CRYPT_INTEGER_BLOB {
            cbData: cipher.len() as u32,
            pbData: cipher.as_ptr() as *mut u8,
        };
        let mut output = CRYPT_INTEGER_BLOB::default();
        CryptUnprotectData(
            &mut input,
            None,
            None,
            None,
            None,
            CRYPTPROTECT_UI_FORBIDDEN,
            &mut output,
        )
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("DPAPI 解密失败: {e}")))?;
        let slice = std::slice::from_raw_parts(output.pbData, output.cbData as usize);
        let out = slice.to_vec();
        free_blob(output);
        Ok(out)
    }
}

#[cfg(windows)]
unsafe fn free_blob(blob: windows::Win32::Security::Cryptography::CRYPT_INTEGER_BLOB) {
    use windows::Win32::Foundation::LocalFree;
    use windows::Win32::Foundation::HLOCAL;
    if !blob.pbData.is_null() {
        let _ = LocalFree(Some(HLOCAL(blob.pbData as _)));
    }
}

pub fn save_session(tokens: &LoginTokens) -> Result<()> {
    let dir = session_dir();
    fs::create_dir_all(&dir)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("无法创建 cloud 目录: {e}")))?;

    #[cfg(windows)]
    {
        let plain = serde_json::to_vec(&SessionContainer {
            encrypted: false,
            payload: String::new(),
            tokens: Some(tokens.clone()),
        })
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("会话序列化失败: {e}")))?;
        if let Ok(cipher) = dpapi_protect(&plain) {
            let container = SessionContainer {
                encrypted: true,
                payload: data_encoding_hex(&cipher),
                tokens: None,
            };
            let text = serde_json::to_string(&container)
                .map_err(|e| Error::new(ErrorCode::Protocol, format!("会话序列化失败: {e}")))?;
            return atomic_write(&session_path(), &text).map_err(|e| {
                Error::new(
                    ErrorCode::Protocol,
                    format!("会话写入失败: {}", e.message()),
                )
            });
        }
        // DPAPI 失败 → 落到明文回退（保持调用方成功语义，cloud.md 记录该机器口径）
    }

    let container = SessionContainer {
        encrypted: false,
        payload: String::new(),
        tokens: Some(tokens.clone()),
    };
    let text = serde_json::to_string(&container)
        .map_err(|e| Error::new(ErrorCode::Protocol, format!("会话序列化失败: {e}")))?;
    atomic_write(&session_path(), &text).map_err(|e| {
        Error::new(
            ErrorCode::Protocol,
            format!("会话写入失败: {}", e.message()),
        )
    })
}

pub fn load_session() -> Result<LoginTokens> {
    let raw = fs::read_to_string(session_path())
        .map_err(|_| Error::new(ErrorCode::CloudNotLoggedIn, "未登录（无会话文件）"))?;
    let container: SessionContainer = serde_json::from_str(&raw)
        .map_err(|e| Error::new(ErrorCode::CloudAuthFailed, format!("会话文件损坏: {e}")))?;
    if !container.encrypted {
        return container
            .tokens
            .ok_or_else(|| Error::new(ErrorCode::CloudAuthFailed, "会话文件缺少 token"));
    }
    #[cfg(windows)]
    {
        let cipher = data_decoding_hex(&container.payload);
        let plain = dpapi_unprotect(&cipher)?;
        let inner: SessionContainer = serde_json::from_slice(&plain)
            .map_err(|e| Error::new(ErrorCode::CloudAuthFailed, format!("会话解密失败: {e}")))?;
        return inner
            .tokens
            .ok_or_else(|| Error::new(ErrorCode::CloudAuthFailed, "会话文件缺少 token"));
    }
    #[cfg(not(windows))]
    Err(Error::new(
        ErrorCode::CloudAuthFailed,
        "本平台不支持加密会话容器，请重新登录",
    ))
}

pub fn clear_session() -> Result<()> {
    let path = session_path();
    if path.exists() {
        fs::remove_file(&path)
            .map_err(|e| Error::new(ErrorCode::Protocol, format!("会话清理失败: {e}")))?;
    }
    Ok(())
}

fn data_encoding_hex(data: &[u8]) -> String {
    data.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(windows)]
fn data_decoding_hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .filter_map(|i| u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tok() -> LoginTokens {
        LoginTokens {
            account_id: "acc".into(),
            email: "a@b.c".into(),
            access_token: "at".into(),
            refresh_token: "rt".into(),
            expires_in_secs: 60,
        }
    }

    #[test]
    fn session_save_load_clear_roundtrip() {
        save_session(&tok()).unwrap();
        let mut refreshed = tok();
        refreshed.access_token = "new-at".into();
        refreshed.refresh_token = "new-rt".into();
        save_session(&refreshed).unwrap();
        let loaded = load_session().unwrap();
        assert_eq!(loaded.access_token, "new-at");
        assert_eq!(loaded.refresh_token, "new-rt");
        assert_eq!(loaded.email, "a@b.c");
        // 登出清会话（保留 state.json 与本地数据——state 文件不在此清理）
        clear_session().unwrap();
        assert!(load_session().is_err());
    }

    #[test]
    fn device_id_stable_and_sized() {
        let a = device_id();
        let b = device_id();
        assert_eq!(a, b);
        assert_eq!(a.len(), 16);
    }

    #[test]
    fn device_metadata_does_not_overwrite_sync_state() {
        let dir = session_dir();
        fs::create_dir_all(&dir).unwrap();
        let state = dir.join("state.json");
        fs::write(
            &state,
            r#"{"entities":{"w":{"base_rev":7}},"last_synced_ms":123}"#,
        )
        .unwrap();
        let _ = device_id();
        let text = fs::read_to_string(&state).unwrap();
        assert!(text.contains("last_synced_ms"));
        assert!(device_meta_path().exists());
    }
}
