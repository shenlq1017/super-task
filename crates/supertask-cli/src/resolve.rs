//! 工作区解析（1.5 §4.2）：`-w/--workspace` > 环境变量 `SUPERTASK_WORKSPACE` >
//! 从 cwd 向上搜索 `supertask.yaml`；找不到 → `NO_WORKSPACE`（码表与 IPC 一致）。

use std::fs;
use std::path::{Path, PathBuf};

use supertask_core::{Error, ErrorCode};

const SPEC_FILE: &str = "supertask.yaml";

/// 解析并校验工作区根（返回已 canonicalize 的目录，且必须含 supertask.yaml）。
pub fn resolve(explicit: Option<&Path>) -> Result<PathBuf, Error> {
    let root = match explicit {
        Some(p) => fs::canonicalize(p).map_err(|e| {
            Error::new(ErrorCode::NoWorkspace, format!("工作区目录无效: {e}"))
        })?,
        None => search_upward()?,
    };
    if !root.join(SPEC_FILE).is_file() {
        return Err(Error::new(
            ErrorCode::NoWorkspace,
            format!(
                "{} 未找到 supertask.yaml（{}）",
                root.display(),
                match std::env::var("SUPERTASK_WORKSPACE") {
                    Ok(v) => format!("SUPERTASK_WORKSPACE={v}"),
                    Err(_) => "未设置 SUPERTASK_WORKSPACE".into(),
                }
            ),
        ));
    }
    // Windows canonicalize 会带 \\?\ verbatim 前缀，展示与锁路径统一去掉
    Ok(supertask_core::sandbox::strip_verbatim(root))
}

fn search_upward() -> Result<PathBuf, Error> {
    let cwd = std::env::current_dir()
        .map_err(|e| Error::new(ErrorCode::NoWorkspace, format!("无法读取 cwd: {e}")))?;
    let mut cur: Option<&Path> = Some(cwd.as_path());
    while let Some(dir) = cur {
        if dir.join(SPEC_FILE).is_file() {
            return fs::canonicalize(dir)
                .map_err(|e| Error::new(ErrorCode::NoWorkspace, format!("工作区目录无效: {e}")));
        }
        cur = dir.parent();
    }
    Err(Error::new(
        ErrorCode::NoWorkspace,
        "cwd 及其上级目录均未找到 supertask.yaml（可设 SUPERTASK_WORKSPACE 或用 -w 指定）",
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_rejects_dir_without_yaml() {
        let tmp = std::env::temp_dir().join(format!("st-cli-res-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        let err = resolve(Some(&tmp)).unwrap_err();
        assert_eq!(err.code(), ErrorCode::NoWorkspace);
        let _ = fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resolve_accepts_dir_with_yaml() {
        let tmp = std::env::temp_dir().join(format!("st-cli-res-ok-{}", std::process::id()));
        let _ = fs::remove_dir_all(&tmp);
        fs::create_dir_all(&tmp).unwrap();
        fs::write(tmp.join("supertask.yaml"), "version: 1\nservices: {}\n").unwrap();
        let root = resolve(Some(&tmp)).unwrap();
        assert!(root.join("supertask.yaml").is_file());
        let _ = fs::remove_dir_all(&tmp);
    }
}
