//! 1.1 IDE 打开：固定候选探测 + 结构化 argv 启动。
//! Spec: `docs/spec/ipc.md` §10.3、`docs/archive/plans/2026-08-27-v1-1-feature-spec.md` §7。
//!
//! 安全边界：只允许命中固定产品名 + 固定 executable 名（PATH 或常见安装位置），
//! 绝不接受调用方传任意路径；启动参数只含工作区根，不拼 shell 字符串。

use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::error::{Error, ErrorCode, Result};

/// 支持的打开目标：资源管理器 / Cursor / IntelliJ IDEA / VS Code。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ide {
    Explorer,
    Cursor,
    Idea,
    Code,
}

impl Ide {
    /// 中文展示名（错误信息用）。
    fn label(self) -> &'static str {
        match self {
            Ide::Explorer => "资源管理器",
            Ide::Cursor => "Cursor",
            Ide::Idea => "IntelliJ IDEA",
            Ide::Code => "VS Code",
        }
    }
}

/// 解析 IPC 传入的 ide 字符串；仅接受 `explorer | cursor | idea | code`。
pub fn parse_ide(s: &str) -> Option<Ide> {
    match s {
        "explorer" => Some(Ide::Explorer),
        "cursor" => Some(Ide::Cursor),
        "idea" => Some(Ide::Idea),
        "code" => Some(Ide::Code),
        _ => None,
    }
}

/// 在固定目录列表中查找第一个存在的可执行文件（可测纯函数）。
/// `exe_names` 依次尝试；无扩展名的自动补 `.exe`。
fn find_in(dirs: &[PathBuf], exe_names: &[&str]) -> Option<PathBuf> {
    for dir in dirs {
        for name in exe_names {
            let mut candidate = dir.join(name);
            if candidate.extension().is_none() {
                candidate.set_extension("exe");
            }
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

/// 遍历 PATH 各目录搜索给定可执行名。
fn find_in_path(exe_names: &[&str]) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = env::split_paths(&env::var_os("PATH").unwrap_or_default()).collect();
    find_in(&dirs, exe_names)
}

/// JetBrains 安装目录：在 `<base>/JetBrains/` 下按固定产品名前缀
/// `IntelliJ IDEA` 匹配（read_dir 单层，不递归全盘），目录名倒序优先新版本，
/// 返回 `<dir>/bin/idea64.exe`。
fn find_jetbrains(base: &Path) -> Option<PathBuf> {
    let entries = fs::read_dir(base.join("JetBrains")).ok()?;
    let mut dirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    dirs.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    for dir in dirs {
        let is_product = dir
            .file_name()
            .map(|n| n.to_string_lossy().starts_with("IntelliJ IDEA"))
            .unwrap_or(false);
        if is_product {
            let exe = dir.join("bin").join("idea64.exe");
            if exe.is_file() {
                return Some(exe);
            }
        }
    }
    None
}

/// 用户级/系统级安装位置：只读环境变量拼固定相对路径。
fn candidate_dirs(ide: Ide) -> Vec<PathBuf> {
    fn push_env(dirs: &mut Vec<PathBuf>, key: &str, rel: &[&str]) {
        if let Some(base) = env::var_os(key) {
            let mut p = PathBuf::from(base);
            for part in rel {
                p.push(part);
            }
            dirs.push(p);
        }
    }
    let mut dirs = Vec::new();
    match ide {
        // explorer 系统必有：PATH 之外再兜底 %SystemRoot%\explorer.exe
        Ide::Explorer => push_env(&mut dirs, "SystemRoot", &[]),
        Ide::Cursor => push_env(&mut dirs, "LOCALAPPDATA", &["Programs", "cursor"]),
        Ide::Code => push_env(
            &mut dirs,
            "LOCALAPPDATA",
            &["Programs", "Microsoft VS Code"],
        ),
        Ide::Idea => {
            push_env(&mut dirs, "ProgramFiles", &["JetBrains"]);
            push_env(&mut dirs, "LOCALAPPDATA", &["Programs", "JetBrains"]);
        }
    }
    dirs
}

/// 固定候选探测：先 PATH 搜索，再常见用户级/系统级安装位置。
/// 未命中返回 `None`（IPC 层转 `IDE_NOT_FOUND`）。
pub fn resolve(ide: Ide) -> Option<PathBuf> {
    // PATH 中的可执行名与安装目录下的文件名（安装目录下文件名带大写）
    let (path_names, file_names): (&[&str], &[&str]) = match ide {
        Ide::Explorer => (&["explorer"], &["explorer"]),
        Ide::Cursor => (&["cursor"], &["Cursor"]),
        Ide::Code => (&["code"], &["Code"]),
        Ide::Idea => (&["idea64"], &["idea64"]),
    };
    if let Some(hit) = find_in_path(path_names) {
        return Some(hit);
    }
    match ide {
        // IDEA 的候选是 `<JetBrains 父目录>/IntelliJ IDEA*/bin/idea64.exe`
        Ide::Idea => candidate_dirs(ide)
            .iter()
            .find_map(|base| find_jetbrains(base)),
        _ => find_in(&candidate_dirs(ide), file_names),
    }
}

/// 结构化 argv：四种目标都只带工作区根一个参数（可测纯函数）。
/// 未来如需给特定 IDE 加 flag，在此按 ide/exe 分派，仍不拼 shell 字符串。
fn argv_for(_ide: Ide, _exe: &Path, root: &Path) -> Vec<OsString> {
    vec![root.as_os_str().to_os_string()]
}

/// 组装并交给启动器打开工作区根目录。
/// 启动器由调用方注入，便于单元测试不拉起真实 Explorer/IDE。
fn open_with_launcher<F>(ide: Ide, root: &Path, launch: F) -> Result<PathBuf>
where
    F: FnOnce(&Path, &[OsString]) -> std::io::Result<()>,
{
    if !root.is_dir() {
        return Err(Error::new(
            ErrorCode::CwdMissing,
            format!("目录不存在: {}", root.display()),
        ));
    }
    let exe = resolve(ide).ok_or_else(|| {
        Error::new(
            ErrorCode::IdeNotFound,
            format!("未找到 {}，请安装后重试或选择其他打开方式", ide.label()),
        )
    })?;
    let argv = argv_for(ide, &exe, root);
    launch(&exe, &argv).map_err(|e| {
        Error::new(
            ErrorCode::Spawn,
            format!("启动 {} 失败: {e}", exe.display()),
        )
    })?;
    Ok(exe)
}

/// 打开工作区根目录。成功仅表示进程已创建（spawn，不等待），不代表 IDE 加载完成。
/// root 不是目录 → `CwdMissing`；未找到固定候选 → `IdeNotFound`；启动失败 → `Spawn`。
pub fn open(ide: Ide, root: &Path) -> Result<PathBuf> {
    open_with_launcher(ide, root, |exe, argv| {
        Command::new(exe).args(argv).spawn().map(|_| ())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 与仓库现有测试一致：临时目录按进程 id + 用例名隔离，结束清理。
    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-ide-test-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parse_ide_roundtrip() {
        for (text, ide) in [
            ("explorer", Ide::Explorer),
            ("cursor", Ide::Cursor),
            ("idea", Ide::Idea),
            ("code", Ide::Code),
        ] {
            assert_eq!(parse_ide(text), Some(ide));
            // serde 往返：序列化仍是同一个小写串
            let serialized = serde_yaml::to_string(&ide).unwrap();
            assert_eq!(serialized.trim(), text);
            assert_eq!(parse_ide(serialized.trim()), Some(ide));
        }
        // 仅接受全小写
        assert_eq!(parse_ide("Explorer"), None);
        assert_eq!(parse_ide(""), None);
    }

    #[test]
    fn argv_only_contains_root() {
        let root = Path::new(r"C:\work\mall");
        for ide in [Ide::Explorer, Ide::Cursor, Ide::Idea, Ide::Code] {
            let argv = argv_for(ide, Path::new("any.exe"), root);
            assert_eq!(argv.len(), 1, "{ide:?} 只允许带 root 一个参数");
            assert_eq!(argv[0], root.as_os_str());
        }
    }

    #[test]
    fn find_in_hit_and_miss() {
        let dir = temp_dir("find");
        fs::write(dir.join("fake-ide.exe"), b"").unwrap();
        // 无扩展名自动补 .exe
        assert_eq!(
            find_in(&[dir.clone()], &["fake-ide"]),
            Some(dir.join("fake-ide.exe"))
        );
        // 已带扩展名的名字原样匹配
        assert_eq!(
            find_in(&[dir.clone()], &["fake-ide.exe"]),
            Some(dir.join("fake-ide.exe"))
        );
        // 未命中
        assert_eq!(find_in(&[dir.clone()], &["nope"]), None);
        assert_eq!(find_in(&[], &["fake-ide"]), None);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_explorer_always_found() {
        // explorer 系统必有（PATH 或 %SystemRoot%）
        assert!(resolve(Ide::Explorer).is_some());
    }

    #[test]
    fn open_explorer_uses_fake_launcher_without_gui_side_effect() {
        // 这个单测不能调用真实 Explorer：spawn 成功不代表 Explorer 已完成异步路径解析。
        // 使用当前目录作为稳定 root，fake launcher 只记录参数，不创建窗口。
        let root = Path::new(".");
        let mut launched: Option<(PathBuf, Vec<OsString>)> = None;
        let path = open_with_launcher(Ide::Explorer, root, |exe, argv| {
            launched = Some((exe.to_path_buf(), argv.to_vec()));
            Ok(())
        })
        .expect("fake launcher 应成功");
        assert!(path.to_string_lossy().to_lowercase().contains("explorer"));
        let (exe, argv) = launched.expect("应记录一次启动");
        assert_eq!(exe, path);
        assert_eq!(argv, vec![root.as_os_str().to_os_string()]);
    }

    #[test]
    fn launcher_error_maps_to_spawn_without_starting_gui() {
        let err = open_with_launcher(Ide::Explorer, Path::new("."), |_exe, _argv| {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fake launcher",
            ))
        })
        .unwrap_err();
        assert_eq!(err.code(), ErrorCode::Spawn);
    }

    #[test]
    fn open_rejects_missing_root() {
        let err = open(Ide::Explorer, Path::new(r"C:\st-definitely-missing-9f3a")).unwrap_err();
        assert_eq!(err.code(), ErrorCode::CwdMissing);
    }
}
