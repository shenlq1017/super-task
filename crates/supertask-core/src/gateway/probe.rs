//! 1.6 网关二进制探测（规格 §6.2）。
//!
//! 解析顺序：`gateway.bin`（显式）→ PATH → 平台已知位置。Windows 只查
//! PATH 与显式 bin（不做注册表扫描/路径猜测）；版本命令 `nginx -v`（stderr）、
//! `caddy version`、`httpd -v`。只探测不代装（路线原则）。

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};
use crate::probe::{find_on_path, ToolProbe};

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct GatewayProbe {
    pub nginx: ToolProbe,
    pub caddy: ToolProbe,
    pub apache: ToolProbe,
}

/// 三家引擎名 → `toolchain.probe` 输出顺序与错误文案用。
pub const GATEWAY_KINDS: &[crate::spec::GatewayKind] = &[
    crate::spec::GatewayKind::Nginx,
    crate::spec::GatewayKind::Caddy,
    crate::spec::GatewayKind::Apache,
];

/// 二进制候选名（find_on_path 自带 PATHEXT 处理，无需重复 .exe）。
fn candidate_names(kind: crate::spec::GatewayKind) -> &'static [&'static str] {
    use crate::spec::GatewayKind::*;
    match kind {
        Nginx => &["nginx"],
        Caddy => &["caddy"],
        // Debian/Ubuntu 的二进制名是 apache2，发行版 httpd 在 /usr/sbin
        Apache => &["httpd", "apache2"],
    }
}

/// 平台已知位置（规格 §6.2；Windows 只认 PATH，返回空）。
fn gateway_known_dirs() -> Vec<PathBuf> {
    #[cfg(any(target_os = "macos", target_os = "linux"))]
    let mut out = Vec::new();
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let out = Vec::new();
    #[cfg(target_os = "macos")]
    {
        out.push(PathBuf::from("/opt/homebrew/bin"));
        out.push(PathBuf::from("/usr/local/bin"));
    }
    #[cfg(target_os = "linux")]
    {
        out.push(PathBuf::from("/usr/sbin"));
        out.push(PathBuf::from("/usr/bin"));
        out.push(PathBuf::from("/snap/bin"));
    }
    out
}

/// 解析单家引擎二进制：显式 bin（必须存在）→ PATH → 已知位置。
pub fn resolve_gateway_bin(
    kind: crate::spec::GatewayKind,
    explicit: Option<&str>,
) -> Result<PathBuf> {
    if let Some(bin) = explicit.map(str::trim).filter(|s| !s.is_empty()) {
        let p = Path::new(bin);
        if p.is_file() {
            return Ok(p.to_path_buf());
        }
        return Err(Error::new(
            ErrorCode::GatewayBinaryMissing,
            format!("gateway.bin 指向的 {} 不存在: {bin}", kind.as_str()),
        ));
    }
    for name in candidate_names(kind) {
        if let Some(p) = find_on_path(name) {
            return Ok(p);
        }
    }
    for dir in gateway_known_dirs() {
        for name in candidate_names(kind) {
            let p = dir.join(name);
            if p.is_file() {
                return Ok(p);
            }
        }
    }
    Err(Error::new(
        ErrorCode::GatewayBinaryMissing,
        format!("未找到 {}。{}", kind.as_str(), install_hint(kind)),
    ))
}

/// 平台对应的一句安装指引（§6.2：缺失时给出，不代装）。
pub fn install_hint(kind: crate::spec::GatewayKind) -> &'static str {
    #[cfg(windows)]
    {
        match kind {
            crate::spec::GatewayKind::Nginx => "Windows：从 nginx.org 下载 zip 解压后把目录加入 PATH，或在 yaml gateway.bin 指定 nginx.exe 路径。",
            crate::spec::GatewayKind::Caddy => "Windows：`winget install CaddyServer.Caddy` 或从 caddyserver.com 下载后加入 PATH。",
            crate::spec::GatewayKind::Apache => "Windows：安装 XAMPP（含 Apache）后把 <apache>/bin 加入 PATH，或在 yaml gateway.bin 指定 httpd.exe 路径。",
        }
    }
    #[cfg(target_os = "macos")]
    {
        match kind {
            crate::spec::GatewayKind::Nginx => "macOS：`brew install nginx`。",
            crate::spec::GatewayKind::Caddy => "macOS：`brew install caddy`。",
            crate::spec::GatewayKind::Apache => "macOS：`brew install httpd`。",
        }
    }
    #[cfg(target_os = "linux")]
    {
        match kind {
            crate::spec::GatewayKind::Nginx => "Linux：`apt install nginx` 或对应发行版包管理器。",
            crate::spec::GatewayKind::Caddy => {
                "Linux：参照 caddyserver.com 安装仓库或 `apt install caddy`。"
            }
            crate::spec::GatewayKind::Apache => {
                "Linux：`apt install apache2` / `dnf install httpd`。"
            }
        }
    }
}

/// 单家探测：解析 → 版本。未解析到 → found=false。
fn probe_kind(kind: crate::spec::GatewayKind, explicit: Option<&str>) -> ToolProbe {
    let Ok(path) = resolve_gateway_bin(kind, explicit) else {
        return ToolProbe::default();
    };
    let args: &[&str] = match kind {
        crate::spec::GatewayKind::Nginx => &["-v"],
        crate::spec::GatewayKind::Caddy => &["version"],
        crate::spec::GatewayKind::Apache => &["-v"],
    };
    match crate::probe::version_of(&path, args) {
        Some(version) => ToolProbe {
            found: true,
            version: Some(version),
            path: Some(path.display().to_string()),
        },
        None => ToolProbe {
            found: false,
            version: None,
            path: Some(path.display().to_string()),
        },
    }
}

/// 三家并行探测（与 toolchain 探测同节奏；总时长受最慢单项约束）。
pub fn probe_gateway() -> GatewayProbe {
    std::thread::scope(|s| {
        let nginx = s.spawn(|| probe_kind(crate::spec::GatewayKind::Nginx, None));
        let caddy = s.spawn(|| probe_kind(crate::spec::GatewayKind::Caddy, None));
        let apache = s.spawn(|| probe_kind(crate::spec::GatewayKind::Apache, None));
        GatewayProbe {
            nginx: nginx.join().unwrap_or_default(),
            caddy: caddy.join().unwrap_or_default(),
            apache: apache.join().unwrap_or_default(),
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::GatewayKind;

    #[test]
    fn resolve_explicit_bin_wins_even_when_missing_errors() {
        let dir = std::env::temp_dir().join(format!("st-gwprobe-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let bin = dir.join("my-nginx");
        std::fs::write(&bin, b"stub").unwrap();
        let resolved =
            resolve_gateway_bin(GatewayKind::Nginx, Some(&bin.display().to_string())).unwrap();
        assert_eq!(resolved, bin);
        // 显式路径不存在 → GATEWAY_BINARY_MISSING（不回落 PATH，避免静默换引擎）
        let e = resolve_gateway_bin(
            GatewayKind::Nginx,
            Some(dir.join("nope").display().to_string().as_str()),
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayBinaryMissing);
        assert!(e.message().contains("gateway.bin"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(unix)]
    #[test]
    fn missing_binary_message_carries_install_hint() {
        // 部分 Linux 环境预装了 nginx（如 GitHub ubuntu runner 自带 /usr/sbin/nginx），
        // 缺失分支无从触发，此时本测试无对象可验。
        if resolve_gateway_bin(GatewayKind::Nginx, None).is_ok() {
            return;
        }
        let e = resolve_gateway_bin(GatewayKind::Nginx, None).unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayBinaryMissing);
        assert!(
            e.message().contains("brew install nginx") || e.message().contains("apt install nginx")
        );
    }

    #[test]
    fn hints_cover_all_kinds_and_platforms() {
        for kind in GATEWAY_KINDS {
            let hint = install_hint(*kind);
            assert!(!hint.is_empty());
        }
    }
}
