//! 镜像构建 argv 组装（规格 §6）。顺序固定：
//! - builds 条目：`docker build -f <dockerfile> -t <tag>... <context>`（无 dockerfile 省略 -f）
//! - compose 服务：`docker compose --ansi never -f <file> [-p <name>] build <service>`
//!
//! context/dockerfile 先过 [`crate::sandbox::confine`] 词法沙箱；存在时再
//! canonicalize 验证仍在工作区内（防 symlink 逃逸）→ `PATH_ESCAPE`。

use std::path::Path;

use crate::error::{Error, ErrorCode, Result};
use crate::spec::DockerBuild;

/// builds 条目 → (argv, dockerfile 相对 root 的显示路径)。argv 不含程序名 `docker`。
pub fn plan_build_entry(root: &Path, b: &DockerBuild) -> Result<Vec<String>> {
    let context = sandbox_path(root, &b.context, "context")?;
    let mut args: Vec<String> = vec!["build".into()];
    if let Some(df) = &b.dockerfile {
        let dockerfile = sandbox_path(root, df, "dockerfile")?;
        args.push("-f".into());
        args.push(dockerfile.display().to_string());
    }
    for t in &b.tags {
        args.push("-t".into());
        args.push(t.clone());
    }
    args.push(context.display().to_string());
    Ok(args)
}

/// compose 服务构建 → argv（不含程序名 `docker`）。
pub fn plan_compose_build(file: &Path, project: Option<&str>, service: &str) -> Vec<String> {
    let mut args = compose_base_args(file, project);
    args.push("build".into());
    args.push(service.into());
    args
}

/// `compose --ansi never -f <file> [-p <name>]` 公共前缀（up/stop/ps/logs/build 共用）。
pub fn compose_base_args(file: &Path, project: Option<&str>) -> Vec<String> {
    let mut args = vec![
        "compose".to_string(),
        "--ansi".to_string(),
        "never".to_string(),
        "-f".to_string(),
        file.display().to_string(),
    ];
    if let Some(p) = project {
        args.push("-p".into());
        args.push(p.into());
    }
    args
}

/// 词法沙箱 + （存在时）canonicalize 复核。任何逃逸 → `PATH_ESCAPE`。
fn sandbox_path(root: &Path, rel: &str, label: &str) -> Result<std::path::PathBuf> {
    let p = crate::sandbox::confine(root, rel)?;
    if let Ok(canon) = std::fs::canonicalize(&p) {
        let canon = crate::sandbox::strip_verbatim(canon);
        let root_c = crate::sandbox::strip_verbatim(
            std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf()),
        );
        if !canon.starts_with(&root_c) {
            return Err(Error::new(
                ErrorCode::PathEscape,
                format!("build {label} 逃出工作区: {rel}"),
            ));
        }
        return Ok(canon);
    }
    // 路径尚不存在：词法沙箱已兜底（compose build / docker build 会给出真实错误）
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::parse_yaml;
    use std::fs;
    use std::path::PathBuf;

    fn temp_ws(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("st-dbuild-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn entry(yaml: &str) -> DockerBuild {
        let text = format!("version: 1\nservices:\n  a:\n    kind: node\n    dir: a\n    port: 3000\ndocker:\n  builds:\n{yaml}");
        let (f, _) = parse_yaml(&text).unwrap();
        f.docker.unwrap().builds.remove(0)
    }

    #[test]
    fn argv_order_fixed_with_and_without_dockerfile() {
        let dir = temp_ws("argv");
        fs::create_dir_all(dir.join("user-service")).unwrap();
        fs::write(dir.join("user-service/Dockerfile"), "FROM scratch\n").unwrap();
        let b = entry(
            "    - name: mall-user\n      context: user-service\n      dockerfile: user-service/Dockerfile\n      tags:\n        - mall-user:local\n        - mall-user:1.0\n",
        );
        let args = plan_build_entry(&dir, &b).unwrap();
        assert_eq!(args[0], "build");
        // -f 与绝对 dockerfile 路径
        assert_eq!(args[1], "-f");
        assert!(
            args[2].ends_with("user-service\\Dockerfile")
                || args[2].ends_with("user-service/Dockerfile")
        );
        // -t 标签按 YAML 顺序
        assert_eq!(
            &args[3..7],
            &["-t", "mall-user:local", "-t", "mall-user:1.0"]
        );
        // context 兜底在最后
        let last = args.last().unwrap();
        assert!(last.ends_with("user-service"));

        // 无 dockerfile：省略 -f
        let b2 = entry("    - name: x\n      context: user-service\n      tags: [x:local]\n");
        let args2 = plan_build_entry(&dir, &b2).unwrap();
        assert!(!args2.contains(&"-f".to_string()));
        assert_eq!(&args2[1..3], &["-t", "x:local"]);
        assert!(args2.last().unwrap().ends_with("user-service"));
    }

    #[test]
    fn compose_build_argv() {
        let args = plan_compose_build(Path::new("C:/w/compose.yaml"), Some("mall"), "redis");
        assert_eq!(
            args,
            vec![
                "compose",
                "--ansi",
                "never",
                "-f",
                "C:/w/compose.yaml",
                "-p",
                "mall",
                "build",
                "redis"
            ]
        );
        let no_project = plan_compose_build(Path::new("C:/w/compose.yaml"), None, "redis");
        assert!(!no_project.contains(&"-p".to_string()));
        assert_eq!(&no_project[no_project.len() - 2..], &["build", "redis"]);
    }

    #[test]
    fn path_escape_inside_and_outside() {
        let dir = temp_ws("escape");
        fs::create_dir_all(dir.join("ctx")).unwrap();
        let ok = entry("    - name: ok\n      context: ctx\n      tags: [a:1]\n");
        assert!(plan_build_entry(&dir, &ok).is_ok());
        // 词法逃逸（`..` 段在 parse_yaml 校验就拒绝，这里直接构造结构体测沙箱层）
        let out = DockerBuild {
            name: "out".into(),
            context: "../outside".into(),
            dockerfile: None,
            tags: vec!["a:1".into()],
            extra: Default::default(),
        };
        assert_eq!(
            plan_build_entry(&dir, &out).unwrap_err().code(),
            ErrorCode::PathEscape
        );
        // symlink 逃逸：canonicalize 后不在工作区内
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/tmp", dir.join("link")).unwrap();
            let link = entry("    - name: link\n      context: link\n      tags: [a:1]\n");
            assert_eq!(
                plan_build_entry(&dir, &link).unwrap_err().code(),
                ErrorCode::PathEscape
            );
        }
    }
}
