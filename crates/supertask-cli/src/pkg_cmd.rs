//! export / import 子命令（1.5 §6.2/§6.3），薄适配 core `pkg` 模块。

use std::path::{Path, PathBuf};

use crate::output;
use supertask_core::pkg;
use supertask_core::Error;

/// 缺省输出名：`supertask-<目录名>-<yyyymmdd-HHmm>.zip`（UTC 时间，避免引日期库）。
pub fn default_export_name(root: &Path) -> String {
    let dir = root
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "workspace".into());
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    let (y, m, d) = civil_from_days(days as i64);
    let rem = secs % 86_400;
    format!(
        "supertask-{dir}-{y:04}{m:02}{d:02}-{:02}{:02}.zip",
        rem / 3600,
        (rem % 3600) / 60
    )
}

/// Howard Hinnant civil_from_days：epoch 天数 → (y, m, d)。
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn run_export(
    json: bool,
    root: &Path,
    dest: Option<&Path>,
    with_secrets: bool,
) -> Result<i32, Error> {
    let dest: PathBuf = match dest {
        Some(p) => p.to_path_buf(),
        None => std::env::current_dir()
            .map_err(|e| {
                Error::new(
                    supertask_core::ErrorCode::NoWorkspace,
                    format!("无法读取 cwd: {e}"),
                )
            })?
            .join(default_export_name(root)),
    };
    let out = pkg::export_package(root, &dest, with_secrets)?;
    if !json {
        println!(
            "已导出 {}（{} 个条目，{} 字节 yaml）",
            out.path.display(),
            out.entries.len(),
            out.entries.first().map(|e| e.bytes).unwrap_or(0),
        );
        for w in &out.warnings {
            println!("  警告: {w}");
        }
        if !with_secrets {
            println!("（默认不含密钥；--with-secrets 打包明文密钥文件）");
        }
    }
    output::ok(
        json,
        serde_json::json!({
            "path": out.path.display().to_string(),
            "entries": out.entries.iter().map(|e| serde_json::json!({"path": e.path, "bytes": e.bytes})).collect::<Vec<_>>(),
            "warnings": out.warnings,
        }),
    );
    Ok(output::EXIT_OK)
}

pub fn run_import(json: bool, pkg_path: &Path, dest: &Path) -> Result<i32, Error> {
    let out = pkg::import_package(pkg_path, dest)?;
    if !json {
        println!("已导入到 {}", out.root.display());
        for w in &out.warnings {
            println!("  警告: {w}");
        }
        println!("下一步：supertask up（或在桌面端打开该目录）");
    }
    output::ok(
        json,
        serde_json::json!({
            "root": out.root.display().to_string(),
            "warnings": out.warnings,
        }),
    );
    Ok(output::EXIT_OK)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_name_shape() {
        let root = Path::new("/tmp/whatever");
        let name = default_export_name(root);
        assert!(name.starts_with("supertask-whatever-"), "{name}");
        assert!(name.ends_with(".zip"), "{name}");
        // yyyymmdd-HHmm 段长度
        let core = name
            .trim_start_matches("supertask-whatever-")
            .trim_end_matches(".zip");
        assert_eq!(core.len(), 13, "{core}");
    }

    /// 跨平台导出→导入全链路（CI 三平台跑，规格 §13.4 迁移用例的自动化部分）。
    #[test]
    fn export_import_round_trip_via_cli_commands() {
        use std::fs;
        let root = std::env::temp_dir().join(format!("st-cli-pkg-{}-ws", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let yaml = "version: 1\nname: rt\nservices:\n  api:\n    kind: spring-boot\n    module: m\n    port: 18090\n    env_file:\n      - .env.shared\nsecrets:\n  backend: file\n  file: .env.local\n";
        fs::write(root.join("supertask.yaml"), yaml).unwrap();
        fs::write(root.join(".env.local"), "DB_PASSWORD=hunter2\n").unwrap();
        fs::write(root.join(".env.shared"), "LOG_LEVEL=info\n").unwrap();

        let zip = root
            .parent()
            .unwrap()
            .join(format!("st-cli-pkg-{}-rt.zip", std::process::id()));
        let imported = root
            .parent()
            .unwrap()
            .join(format!("st-cli-pkg-{}-in", std::process::id()));
        let _ = fs::remove_dir_all(&imported);

        run_export(false, &root, Some(&zip), true).unwrap();
        run_import(false, &zip, &imported).unwrap();

        assert_eq!(
            fs::read(imported.join("supertask.yaml")).unwrap(),
            yaml.as_bytes()
        );
        assert_eq!(
            fs::read_to_string(imported.join(".env.local")).unwrap(),
            "DB_PASSWORD=hunter2\n"
        );
        // 运行时产物不进包
        assert!(!imported.join(".supertask").exists());

        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_file(&zip);
        let _ = fs::remove_dir_all(&imported);
    }
}
