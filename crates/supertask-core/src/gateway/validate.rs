//! 1.6 校验链执行器（规格 §6.1）。
//!
//! 生成 → 落盘 → spawn 只读校验命令（`nginx -t` / `caddy validate` /
//! `httpd -t`，10s 超时）→ 非零退出 `GATEWAY_CONFIG_INVALID`（details 带
//! stdout/stderr 原文）；spawn 失败（二进制缺失）→ `GATEWAY_BINARY_MISSING`。
//! runner 可注入（测试用脚本桩，不拉真反代）。

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use crate::error::{Error, ErrorCode, Result};
use crate::spec::GatewayKind;

use super::model::ResolvedGateway;

/// 单次校验超时（§6.1：校验是只读命令，10s 上限）。
pub const VALIDATE_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidateOutcome {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// 校验命令执行器（可注入 fake；生产 spawn 真进程）。
pub trait ValidateRunner: Send + Sync {
    fn run(&self, program: &Path, args: &[String], timeout: Duration) -> Result<ValidateOutcome>;
}

#[derive(Debug, Default)]
pub struct ProcessValidateRunner;

impl ValidateRunner for ProcessValidateRunner {
    fn run(&self, program: &Path, args: &[String], timeout: Duration) -> Result<ValidateOutcome> {
        let mut cmd = Command::new(program);
        cmd.args(args)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .stdin(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
        }
        let mut child = cmd.spawn().map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                Error::new(
                    ErrorCode::GatewayBinaryMissing,
                    format!("无法启动校验进程 {}: {e}", program.display()),
                )
            } else {
                Error::new(
                    ErrorCode::GatewayConfigInvalid,
                    format!("无法启动校验进程 {}: {e}", program.display()),
                )
            }
        })?;
        let deadline = Instant::now() + timeout;
        loop {
            match child.try_wait().map_err(|e| {
                Error::new(
                    ErrorCode::GatewayConfigInvalid,
                    format!("等待校验进程失败: {e}"),
                )
            })? {
                Some(_) => {
                    let out = child.wait_with_output().map_err(|e| {
                        Error::new(
                            ErrorCode::GatewayConfigInvalid,
                            format!("读取校验输出失败: {e}"),
                        )
                    })?;
                    return Ok(ValidateOutcome {
                        code: out.status.code().unwrap_or(-1),
                        stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
                        stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
                    });
                }
                None => {
                    if Instant::now() >= deadline {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(Error::new(
                            ErrorCode::GatewayConfigInvalid,
                            format!("校验命令超时（{}s）", timeout.as_secs()),
                        ));
                    }
                    std::thread::sleep(Duration::from_millis(40));
                }
            }
        }
    }
}

/// 产物目录：`<root>/.supertask/gateway/`。
pub fn gateway_dir(root: &Path) -> PathBuf {
    root.join(".supertask").join("gateway")
}

/// 产物文件名。
pub fn conf_file_name(kind: GatewayKind) -> &'static str {
    match kind {
        GatewayKind::Nginx => "nginx.conf",
        GatewayKind::Caddy => "Caddyfile",
        GatewayKind::Apache => "httpd.conf",
    }
}

/// 渲染产物落盘（磁盘产物是缓存，不是编辑对象）。返回 conf 绝对路径。
pub fn write_conf(root: &Path, ir: &ResolvedGateway) -> Result<PathBuf> {
    let dir = gateway_dir(root);
    std::fs::create_dir_all(&dir).map_err(|e| {
        Error::new(
            ErrorCode::GatewayConfigInvalid,
            format!("无法创建 {}: {e}", dir.display()),
        )
    })?;
    // apache 模块目录：引擎侧由 bin 位置注入（bin 同级 modules/，XAMPP 与
    // 官方 zip 布局一致）；未注入时回落产物目录内 modules（校验会原文报错）
    let modules_dir = ir
        .apache_modules_dir
        .clone()
        .unwrap_or_else(|| dir.join("modules").to_string_lossy().into_owned());
    let (name, content) = super::render::render_conf(ir, &dir.to_string_lossy(), &modules_dir)?;
    let conf = dir.join(name);
    std::fs::write(&conf, content).map_err(|e| {
        Error::new(
            ErrorCode::GatewayConfigInvalid,
            format!("写入 {} 失败: {e}", conf.display()),
        )
    })?;
    Ok(conf)
}

/// 校验 argv（§6.1）：`nginx -t -c <conf> -p <prefix> -e stderr` /
/// `caddy validate --config <conf> --adapter caddyfile` / `httpd -t -f <conf>`。
pub fn validate_argv(kind: GatewayKind, conf: &Path, prefix: &Path) -> Vec<String> {
    let conf = conf.to_string_lossy().into_owned();
    let prefix = format!(
        "{}/",
        prefix
            .to_string_lossy()
            .replace('\\', "/")
            .trim_end_matches('/')
    );
    match kind {
        GatewayKind::Nginx => vec![
            "-t".into(),
            "-c".into(),
            conf,
            "-p".into(),
            prefix,
            "-e".into(),
            "stderr".into(),
        ],
        GatewayKind::Caddy => vec![
            "validate".into(),
            "--config".into(),
            conf,
            "--adapter".into(),
            "caddyfile".into(),
        ],
        GatewayKind::Apache => vec!["-t".into(), "-f".into(), conf],
    }
}

/// 启动 argv（§5）：nginx 前台 `daemon off`；caddy `run`；apache 平台分支
/// （Unix `-DFOREGROUND`；Windows 父子进程由 Job Object 收编，不加该参数）。
pub fn start_argv(kind: GatewayKind, conf: &Path, prefix: &Path) -> Vec<String> {
    match kind {
        GatewayKind::Nginx => validate_argv(kind, conf, prefix)
            .into_iter()
            .filter(|s| s != "-t")
            .collect::<Vec<_>>(),
        GatewayKind::Caddy => vec![
            "run".into(),
            "--config".into(),
            conf.to_string_lossy().into_owned(),
            "--adapter".into(),
            "caddyfile".into(),
        ],
        GatewayKind::Apache => {
            #[cfg(windows)]
            {
                vec!["-f".into(), conf.to_string_lossy().into_owned()]
            }
            #[cfg(not(windows))]
            {
                vec![
                    "-DFOREGROUND".into(),
                    "-f".into(),
                    conf.to_string_lossy().into_owned(),
                ]
            }
        }
    }
}

/// `caddy trust`（仅 UI 显式确认后由引擎调用；修改系统信任库）。
pub fn trust_argv() -> Vec<String> {
    vec!["trust".into()]
}

/// 完整校验链：渲染落盘 → spawn 校验 → 错误映射（§6.1 第 2/3 步；
/// 静态校验与二进制探测由引擎在调用前完成）。
pub fn validate_gateway(
    root: &Path,
    ir: &ResolvedGateway,
    bin: &Path,
    runner: &dyn ValidateRunner,
) -> Result<PathBuf> {
    let conf = write_conf(root, ir)?;
    let prefix = gateway_dir(root);
    let args = validate_argv(ir.kind, &conf, &prefix);
    let outcome = runner.run(bin, &args, VALIDATE_TIMEOUT)?;
    if outcome.code == 0 {
        return Ok(conf);
    }
    let stderr = outcome.stderr.trim();
    let stdout = outcome.stdout.trim();
    let mut detail = String::new();
    if !stderr.is_empty() {
        detail.push_str(stderr);
    }
    if !stdout.is_empty() {
        if !detail.is_empty() {
            detail.push('\n');
        }
        detail.push_str(stdout);
    }
    let message = if detail.is_empty() {
        format!("{} 校验未通过（退出码 {}）", ir.kind.as_str(), outcome.code)
    } else {
        format!("{} 校验未通过：\n{detail}", ir.kind.as_str())
    };
    Err(
        Error::new(ErrorCode::GatewayConfigInvalid, message).details(
            serde_yaml::to_value(&serde_yaml::Mapping::from_iter([
                (
                    serde_yaml::Value::String("stderr".into()),
                    serde_yaml::Value::String(stderr.into()),
                ),
                (
                    serde_yaml::Value::String("stdout".into()),
                    serde_yaml::Value::String(stdout.into()),
                ),
                (
                    serde_yaml::Value::String("engine".into()),
                    serde_yaml::Value::String(ir.kind.as_str().into()),
                ),
            ]))
            .unwrap_or(serde_yaml::Value::Null),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct FakeValidate {
        outcomes: Mutex<Vec<std::result::Result<ValidateOutcome, String>>>,
    }

    impl FakeValidate {
        fn ok() -> Self {
            Self {
                outcomes: Mutex::new(vec![Ok(ValidateOutcome {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                })]),
            }
        }
        fn fail(code: i32, stderr: &str) -> Self {
            Self {
                outcomes: Mutex::new(vec![Ok(ValidateOutcome {
                    code,
                    stdout: String::new(),
                    stderr: stderr.into(),
                })]),
            }
        }
    }

    impl ValidateRunner for FakeValidate {
        fn run(
            &self,
            _program: &Path,
            _args: &[String],
            _timeout: Duration,
        ) -> Result<ValidateOutcome> {
            self.outcomes
                .lock()
                .unwrap()
                .pop()
                .expect("未排队的结果")
                .map_err(|msg| Error::new(ErrorCode::GatewayBinaryMissing, msg))
        }
    }

    fn ws_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("st-gwval-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn ir_of(gateway_yaml: &str) -> ResolvedGateway {
        let text = format!(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n    module: api\n    port: 8081\ngateway:\n{gateway_yaml}"
        );
        let (f, _) = crate::spec::parse_yaml(&text).unwrap();
        let conf = f.gateway.clone().unwrap();
        crate::gateway::model::resolve(&f, &conf, &|_| "127.0.0.1".into(), "").unwrap()
    }

    #[test]
    fn validate_ok_writes_conf_file() {
        let root = ws_root("ok");
        let ir =
            ir_of("  kind: nginx\n  port: 8080\n  routes:\n    - path: /\n      target: api\n");
        let conf = validate_gateway(
            &root,
            &ir,
            Path::new("C:/bin/nginx.exe"),
            &FakeValidate::ok(),
        )
        .unwrap();
        assert_eq!(conf, gateway_dir(&root).join("nginx.conf"));
        let text = std::fs::read_to_string(&conf).unwrap();
        assert!(text.contains("proxy_pass http://127.0.0.1:8081;"), "{text}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_nonzero_maps_to_config_invalid_with_stderr() {
        let root = ws_root("fail");
        let ir = ir_of("  kind: nginx\n  port: 8080\n");
        let e = validate_gateway(
            &root,
            &ir,
            Path::new("nginx"),
            &FakeValidate::fail(1, "[emerg] bind() to 0.0.0.0:8080 failed"),
        )
        .unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayConfigInvalid);
        assert!(e.message().contains("[emerg]"), "{}", e);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn validate_caddy_and_apache_argv() {
        let root = ws_root("argv");
        let ir_c = ir_of("  kind: caddy\n  port: 8443\n  tls: internal\n");
        let conf_c = write_conf(&root, &ir_c).unwrap();
        assert!(conf_c.ends_with("Caddyfile"));
        let a = validate_argv(GatewayKind::Caddy, &conf_c, &gateway_dir(&root));
        assert_eq!(
            a,
            vec![
                "validate".to_string(),
                "--config".to_string(),
                conf_c.to_string_lossy().into_owned(),
                "--adapter".to_string(),
                "caddyfile".to_string(),
            ]
        );

        let ir_a = ir_of("  kind: apache\n  port: 8080\n");
        let conf_a = write_conf(&root, &ir_a).unwrap();
        let a = validate_argv(GatewayKind::Apache, &conf_a, &gateway_dir(&root));
        assert_eq!(a[0], "-t");
        assert_eq!(a[1], "-f");

        let n = validate_argv(GatewayKind::Nginx, &conf_a, &gateway_dir(&root));
        assert_eq!(
            &n[0..3],
            &[
                "-t".to_string(),
                "-c".to_string(),
                conf_a.to_string_lossy().into_owned()
            ]
        );
        assert!(n.contains(&"-p".to_string()) && n.contains(&"-e".to_string()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn start_argv_shapes() {
        let conf = Path::new("C:/ws/.supertask/gateway/nginx.conf");
        let prefix = Path::new("C:/ws/.supertask/gateway");
        let n = start_argv(GatewayKind::Nginx, conf, prefix);
        let n = n;
        assert!(!n.contains(&"-t".to_string()), "{n:?}");
        assert!(n.contains(&"-e".to_string()));
        let c = start_argv(GatewayKind::Caddy, conf, prefix);
        assert_eq!(c[0], "run");
        let a = start_argv(GatewayKind::Apache, conf, prefix);
        #[cfg(windows)]
        assert_eq!(
            a,
            vec!["-f".to_string(), conf.to_string_lossy().into_owned()]
        );
        #[cfg(not(windows))]
        assert_eq!(a[0], "-DFOREGROUND");
    }

    #[test]
    fn validate_binary_missing_propagates() {
        let root = ws_root("missing");
        let ir = ir_of("  kind: nginx\n  port: 8080\n");
        let fake = FakeValidate {
            outcomes: Mutex::new(vec![Err("not found".into())]),
        };
        let e = validate_gateway(&root, &ir, Path::new("nginx"), &fake).unwrap_err();
        assert_eq!(e.code(), ErrorCode::GatewayBinaryMissing);
        let _ = std::fs::remove_dir_all(&root);
    }
}
