use std::path::{Component, Path, PathBuf};

use crate::error::{Error, ErrorCode, Result};

/// Relative path must stay inside the workspace. No absolute, no `..` escape.
pub fn assert_rel_safe(rel: &str) -> Result<PathBuf> {
    if rel.is_empty() {
        return Err(Error::new(ErrorCode::PathEscape, "路径为空"));
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(Error::new(
            ErrorCode::PathEscape,
            format!("禁止绝对路径: {rel}"),
        ));
    }
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::Normal(s) => out.push(s),
            Component::ParentDir => {
                if !out.pop() {
                    return Err(Error::new(
                        ErrorCode::PathEscape,
                        format!("路径逃出工作区: {rel}"),
                    ));
                }
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(Error::new(
                    ErrorCode::PathEscape,
                    format!("非法路径: {rel}"),
                ));
            }
        }
    }
    Ok(out)
}

pub fn confine(root: &Path, rel: &str) -> Result<PathBuf> {
    let rel_n = assert_rel_safe(rel)?;
    let joined = root.join(&rel_n);
    if let (Ok(root_c), Ok(joined_c)) = (dunce_canon(root), dunce_canon(&joined)) {
        if !joined_c.starts_with(&root_c) {
            return Err(Error::new(ErrorCode::PathEscape, "路径逃出工作区"));
        }
        return Ok(joined_c);
    }
    Ok(joined)
}

fn dunce_canon(p: &Path) -> std::io::Result<PathBuf> {
    let c = std::fs::canonicalize(p)?;
    Ok(strip_verbatim(c))
}

/// Remove the Windows verbatim prefix (`\\?\C:\…` → `C:\…`) that
/// `std::fs::canonicalize` emits. The prefix is semantically identical for
/// drive paths, but leaking it into UI strings (workspace_id 等) looks wrong.
pub fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        // UNC 形式 \\?\UNC\server\share → 还原成网络路径 \\server\share
        if let Some(unc) = rest.strip_prefix(r"UNC\") {
            return PathBuf::from(format!(r"\\{unc}"));
        }
        PathBuf::from(rest)
    } else {
        p
    }
}

pub fn is_loopback_url(url: &str) -> bool {
    let Some(rest) = url
        .strip_prefix("http://")
        .or_else(|| url.strip_prefix("https://"))
    else {
        return false;
    };
    let host = rest
        .split('/')
        .next()
        .unwrap_or("")
        .split(':')
        .next()
        .unwrap_or("");
    matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blocks_escape() {
        assert!(assert_rel_safe("../etc").is_err());
        assert!(assert_rel_safe("C:\\Windows").is_err());
        assert!(assert_rel_safe("web/../web").is_ok());
        assert!(assert_rel_safe("web/app").is_ok());
    }

    #[test]
    fn loopback_only() {
        assert!(is_loopback_url("http://127.0.0.1:8080/actuator/health"));
        assert!(is_loopback_url("http://localhost:1/"));
        assert!(!is_loopback_url("http://example.com/x"));
        assert!(!is_loopback_url("http://10.0.0.1/x"));
    }

    #[test]
    fn strips_verbatim_prefix() {
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\C:\project\demo")),
            PathBuf::from(r"C:\project\demo")
        );
        // 非前缀路径原样保留
        assert_eq!(
            strip_verbatim(PathBuf::from(r"C:\project\demo")),
            PathBuf::from(r"C:\project\demo")
        );
        // UNC 逐字形式还原为网络路径
        assert_eq!(
            strip_verbatim(PathBuf::from(r"\\?\UNC\server\share\dir")),
            PathBuf::from(r"\\server\share\dir")
        );
    }
}
