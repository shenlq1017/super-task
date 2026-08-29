use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
use crate::sandbox;
use crate::spec::{PackageManager, ServiceSpec, SuperTaskFile};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub cwd_rel: String,
    pub env: IndexMap<String, String>,
}

// ---------------------------------------------------------------------------
// 1.4 §5.1 build_tool：maven | gradle，缺省按构建文件探测
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildTool {
    Maven,
    Gradle,
}

impl BuildTool {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Maven => "maven",
            Self::Gradle => "gradle",
        }
    }
}

/// 解析 `build_tool` 字段中的显式值（validate 已拒绝非法值；此处容错为 None）。
pub fn explicit_build_tool(svc: &ServiceSpec) -> Option<BuildTool> {
    match svc.build_tool.as_deref() {
        Some("maven") => Some(BuildTool::Maven),
        Some("gradle") => Some(BuildTool::Gradle),
        _ => None,
    }
}

/// §5.1 探测：module 目录（单模块工程为 root）有 build.gradle(.kts) → gradle；
/// 有 pom.xml → maven；两者并存 → `BUILD_TOOL_AMBIGUOUS`；都没有 → `MISSING_TOOL`。
pub fn detect_build_tool(dir: &Path) -> Result<BuildTool> {
    let gradle = dir.join("build.gradle").is_file() || dir.join("build.gradle.kts").is_file();
    let maven = dir.join("pom.xml").is_file();
    match (maven, gradle) {
        (true, true) => Err(Error::new(
            ErrorCode::BuildToolAmbiguous,
            format!(
                "{} 同时存在 pom.xml 与 build.gradle，无法确定构建工具；请在 supertask.yaml 显式指定 build_tool",
                dir.display()
            ),
        )),
        (true, false) => Ok(BuildTool::Maven),
        (false, true) => Ok(BuildTool::Gradle),
        (false, false) => Err(Error::new(
            ErrorCode::MissingTool,
            format!(
                "{} 未找到构建文件（pom.xml / build.gradle / build.gradle.kts）",
                dir.display()
            ),
        )),
    }
}

/// module 字段 → 工作区内目录（路径逃逸 → PATH_ESCAPE，复用 1.2 沙箱规则）。
fn module_dir(root: &Path, module: &str) -> Result<PathBuf> {
    if module == "." {
        return Ok(root.to_path_buf());
    }
    crate::sandbox::confine(root, module)
}

/// 启动期解析构建工具：显式 build_tool 跳过探测（§5.1）；缺省按文件探测。
pub fn resolve_build_tool(root: &Path, svc: &ServiceSpec) -> Result<BuildTool> {
    if let Some(bt) = explicit_build_tool(svc) {
        return Ok(bt);
    }
    let dir = module_dir(root, svc.module.as_deref().unwrap_or("."))?;
    detect_build_tool(&dir)
}

fn gradle_wrapper_file_name() -> &'static str {
    #[cfg(windows)]
    {
        "gradlew.bat"
    }
    #[cfg(not(windows))]
    {
        "gradlew"
    }
}

/// launcher 计划出的 gradle 程序名（尚未解析 wrapper/PATH）。
pub fn is_gradle_program(program: &str) -> bool {
    program == "gradlew" || program == "gradlew.bat"
}

/// 无执行位警告只发一次（§4.2「警告一次」）。
#[cfg(unix)]
static GRADLE_SH_WARNED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// §5.1 wrapper 优先：root（或 module 目录）存在 gradlew[.bat] 则用 wrapper；
/// 否则用 PATH 解析的 `gradle`；都无 → `GRADLE_WRAPPER_MISSING`（details 建议
/// `gradle wrapper`）。返回 (program, args, warnings)。
pub fn resolve_gradle_launcher(
    root: &Path,
    module: &str,
    program: &str,
    args: &[String],
) -> Result<(String, Vec<String>, Vec<String>)> {
    debug_assert!(is_gradle_program(program));
    let warnings: Vec<String> = Vec::new();
    let mut candidates: Vec<PathBuf> = vec![root.join(gradle_wrapper_file_name())];
    if module != "." {
        if let Ok(dir) = module_dir(root, module) {
            candidates.push(dir.join(gradle_wrapper_file_name()));
        }
    }
    for wrapper in candidates {
        if !wrapper.is_file() {
            continue;
        }
        #[cfg(windows)]
        {
            return Ok((wrapper.display().to_string(), args.to_vec(), warnings));
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let exec = std::fs::metadata(&wrapper)
                .map(|m| m.permissions().mode() & 0o111 != 0)
                .unwrap_or(false);
            if exec {
                return Ok((wrapper.display().to_string(), args.to_vec(), warnings));
            }
            // 无执行位：经 `sh gradlew` 执行并警告一次，不静默失败（§4.2）
            let mut out = vec![wrapper.display().to_string()];
            out.extend(args.iter().cloned());
            if !GRADLE_SH_WARNED.swap(true, std::sync::atomic::Ordering::SeqCst) {
                let mut warns = warnings;
                warns.push(format!(
                    "{} 无执行位，改用 `sh gradlew` 执行；建议 chmod +x gradlew",
                    wrapper.display()
                ));
                return Ok(("sh".into(), out, warns));
            }
            return Ok(("sh".into(), out, warnings));
        }
    }
    if let Some(p) = crate::probe::find_on_path("gradle") {
        return Ok((p.display().to_string(), args.to_vec(), warnings));
    }
    Err(Error::new(
        ErrorCode::GradleWrapperMissing,
        "未找到 Gradle wrapper（gradlew / gradlew.bat），PATH 中也没有 gradle。请在工程根执行 `gradle wrapper --gradle-version <x>` 生成 wrapper，或安装 Gradle 并加入 PATH。",
    )
    .details(serde_yaml::to_value("gradle wrapper").unwrap_or(serde_yaml::Value::Null)))
}

/// Build argv from spec only. Never takes cmdline from IPC.
pub fn plan_service(file: &SuperTaskFile, id: &str) -> Result<CommandSpec> {
    plan_service_in(file, id, None)
}

/// 带 root 的规划：spring-boot 缺省 build_tool 时按构建文件探测（§5.1）。
/// root=None（纯 spec 上下文，如单测 / 测试 spawner）只认显式字段，缺省按 maven。
pub fn plan_service_in(file: &SuperTaskFile, id: &str, root: Option<&Path>) -> Result<CommandSpec> {
    let svc = file
        .services
        .get(id)
        .ok_or_else(|| Error::new(ErrorCode::NotFound, format!("没有服务 {id}")))?;
    if !svc.enabled {
        return Err(Error::new(
            ErrorCode::FeatureDisabled,
            format!("{id} 已 enabled: false"),
        ));
    }
    if !SuperTaskFile::runnable_kind(&svc.kind) {
        return Err(Error::new(
            ErrorCode::KindUnsupported,
            format!("{} 本版不能启动 kind={}", id, svc.kind),
        ));
    }
    let env = merge_env(&file.env, svc);
    match svc.kind.as_str() {
        "spring-boot" => {
            let bt = match root {
                Some(r) => resolve_build_tool(r, svc)?,
                None => explicit_build_tool(svc).unwrap_or(BuildTool::Maven),
            };
            plan_spring(svc, env, bt)
        }
        "node" => plan_node(svc, env),
        // 1.7 §4.2：python/go/generic；root=None（测试上下文）只认显式字段、跳过 fs 检查
        "python" => plan_python(svc, env, root),
        "go" => plan_go(svc, env, root),
        "generic" => plan_generic(svc, env, root),
        _ => unreachable!(),
    }
}

fn merge_env(ws: &IndexMap<String, String>, svc: &ServiceSpec) -> IndexMap<String, String> {
    let mut env = ws.clone();
    for (k, v) in &svc.env {
        env.insert(k.clone(), v.clone());
    }
    if let Some(port) = svc.port {
        match svc.kind.as_str() {
            "spring-boot" if !env.contains_key("SERVER_PORT") => {
                env.insert("SERVER_PORT".into(), port.to_string());
            }
            // 1.7 §4.4：python/go 与 node 同口径注入 PORT；generic 无生态约定不注入
            "node" | "python" | "go" if !env.contains_key("PORT") => {
                env.insert("PORT".into(), port.to_string());
            }
            _ => {}
        }
    }
    env
}

fn plan_spring(
    svc: &ServiceSpec,
    env: IndexMap<String, String>,
    bt: BuildTool,
) -> Result<CommandSpec> {
    let launch = svc.launch.as_deref().unwrap_or("run");
    if launch != "run" {
        return Err(Error::new(
            ErrorCode::LaunchUnsupported,
            format!("launch '{launch}' 本版尚未实现启动"),
        ));
    }
    let module = svc.module.as_deref().unwrap();
    match bt {
        BuildTool::Maven => {
            // `-am` runs spring-boot:run on every reactor project, including aggregator
            // POMs that have no plugin. Also-make belongs in extra_args or bootstrap.
            let mut args = if module == "." {
                vec!["spring-boot:run".into()]
            } else {
                vec!["-pl".into(), module.into(), "spring-boot:run".into()]
            };
            args.extend(svc.extra_args.iter().cloned());
            Ok(CommandSpec {
                program: "mvn.cmd".into(),
                args,
                cwd_rel: svc.cwd.clone().unwrap_or_else(|| ".".into()),
                env,
            })
        }
        BuildTool::Gradle => {
            // 1.4 §5.2：`gradlew [:module:]bootRun`；module="." 省略任务路径前缀。
            // Gradle 自身解析跨模块任务依赖，无 -pl/-am 问题。
            let mut args = gradle_task_args(module, "bootRun");
            args.extend(svc.extra_args.iter().cloned());
            Ok(CommandSpec {
                program: gradle_wrapper_file_name().into(),
                args,
                cwd_rel: svc.cwd.clone().unwrap_or_else(|| ".".into()),
                env,
            })
        }
    }
}

/// `:module:task`；module="." 直接 `task`。module 目录路径 `a/b` → Gradle 项目路径 `:a:b`。
fn gradle_task_args(module: &str, task: &str) -> Vec<String> {
    if module == "." {
        vec![task.to_string()]
    } else {
        vec![format!(":{}:{task}", module.replace('/', ":"))]
    }
}

fn plan_node(svc: &ServiceSpec, env: IndexMap<String, String>) -> Result<CommandSpec> {
    let pm = match svc.package_manager.unwrap_or(PackageManager::Npm) {
        PackageManager::Npm => "npm.cmd",
        PackageManager::Pnpm => "pnpm.cmd",
        PackageManager::Yarn => "yarn.cmd",
    };
    let script = svc.script.clone().unwrap_or_else(|| "dev".into());
    let mut args = vec!["run".into(), script];
    if !svc.extra_args.is_empty() {
        args.push("--".into());
        args.extend(svc.extra_args.iter().cloned());
    }
    Ok(CommandSpec {
        program: pm.into(),
        args,
        cwd_rel: svc.dir.clone().unwrap(),
        env,
    })
}

// ---------------------------------------------------------------------------
// 1.7 §4.2：python / go / generic
// ---------------------------------------------------------------------------

fn go_program_name() -> &'static str {
    if cfg!(windows) {
        "go.exe"
    } else {
        "go"
    }
}

fn python_program_name() -> &'static str {
    if cfg!(windows) {
        "python.exe"
    } else {
        "python3"
    }
}

fn venv_relative_python() -> &'static str {
    if cfg!(windows) {
        "Scripts/python.exe"
    } else {
        "bin/python"
    }
}

/// 1.7 §4.2：python 解释器解析 `dir/.venv → dir/venv → root/.venv → root/venv → PATH`。
/// `.venv` 与 `venv` 并存取 `.venv`。root=None（测试上下文）只走 PATH。
fn resolve_python_interpreter(dir: &str, root: Option<&Path>) -> String {
    let Some(r) = root else {
        return python_program_name().into();
    };
    let dir_abs = sandbox::confine(r, dir).unwrap_or_else(|_| r.to_path_buf());
    let rel = venv_relative_python();
    for (venv, _) in [(".venv", true), ("venv", false)] {
        let p = dir_abs.join(venv).join(rel);
        if p.is_file() {
            return p.to_string_lossy().into_owned();
        }
    }
    python_program_name().into()
}

/// `kind: python`：`python <entry>` 或 `python -m <module>` + extra_args；工作目录 dir。
fn plan_python(
    svc: &ServiceSpec,
    env: IndexMap<String, String>,
    root: Option<&Path>,
) -> Result<CommandSpec> {
    let dir = svc.dir.clone().unwrap();
    let program = resolve_python_interpreter(&dir, root);
    if let (Some(r), Some(entry)) = (root, svc.entry.as_deref()) {
        let base = sandbox::confine(r, &dir)?;
        let p = sandbox::confine(&base, entry)?;
        if !p.is_file() {
            return Err(Error::new(
                ErrorCode::EntryNotFound,
                format!("入口文件不存在: {entry}（相对 dir {dir}）"),
            ));
        }
    }
    let mut args = match (svc.entry.as_deref(), svc.module.as_deref()) {
        (Some(e), _) => vec![e.to_string()],
        (None, Some(m)) => vec!["-m".into(), m.to_string()],
        // validate 已保证 entry XOR module；此处兜底
        (None, None) => {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                "python 需要 entry 或 module",
            ))
        }
    };
    args.extend(svc.extra_args.iter().cloned());
    Ok(CommandSpec {
        program,
        args,
        cwd_rel: dir,
        env,
    })
}

/// `kind: go`：`go run <package>` + extra_args（extra_args 传给被运行程序）；
/// package 缺省 "."；非 `.` 开头的裸路径归一为 `./<package>`；工作目录 dir（缺省 "."）。
fn plan_go(
    svc: &ServiceSpec,
    env: IndexMap<String, String>,
    root: Option<&Path>,
) -> Result<CommandSpec> {
    let dir = svc.dir.clone().unwrap_or_else(|| ".".into());
    let package = svc.package.clone().unwrap_or_else(|| ".".into());
    if let Some(r) = root {
        let bare = package.trim_start_matches("./");
        if !bare.is_empty() && bare != "." {
            let base = sandbox::confine(r, &dir)?;
            let p = sandbox::confine(&base, bare)?;
            if !p.is_dir() {
                return Err(Error::new(
                    ErrorCode::PackageNotFound,
                    format!("package 目录不存在: {package}（相对 dir {dir}）"),
                ));
            }
        }
    }
    let pkg_arg = if package.starts_with('.') {
        package
    } else {
        format!("./{package}")
    };
    let mut args = vec!["run".into(), pkg_arg];
    args.extend(svc.extra_args.iter().cloned());
    Ok(CommandSpec {
        program: go_program_name().into(),
        args,
        cwd_rel: dir,
        env,
    })
}

/// `kind: generic`：`<program> [args…]` + extra_args；program 含路径分隔符时为
/// 工作区内相对路径（相对 dir，validate 已拒绝对 `..`/绝对路径），规划期解析为
/// 绝对路径并检查存在性；纯名字走 PATH（PATHEXT）。UI 不提供拼 cmdline 入口。
fn plan_generic(
    svc: &ServiceSpec,
    env: IndexMap<String, String>,
    root: Option<&Path>,
) -> Result<CommandSpec> {
    let program_spec = svc.program.clone().unwrap();
    let dir = svc.dir.clone().unwrap_or_else(|| ".".into());
    let program = if program_spec.contains('/') || program_spec.contains('\\') {
        match root {
            Some(r) => {
                let base = sandbox::confine(r, &dir)?;
                let p = sandbox::confine(&base, &program_spec)?;
                if !p.is_file() {
                    return Err(Error::new(
                        ErrorCode::MissingTool,
                        format!("未找到程序 {program_spec}（相对 {dir}）"),
                    ));
                }
                p.to_string_lossy().into_owned()
            }
            // 测试上下文：原样透传，由 resolve_program 兜底
            None => program_spec,
        }
    } else {
        program_spec
    };
    let mut args = svc.args.clone();
    args.extend(svc.extra_args.iter().cloned());
    Ok(CommandSpec {
        program,
        args,
        cwd_rel: dir,
        env,
    })
}

// ---------------------------------------------------------------------------
// 1.7 §5（Phase 2.0）：显式 `toolchain.manager: mise` 时的 env_delta 接线。
// resolver 产出的 `env_delta`（mise 工具的 PATH 前置）此前无消费方——在启动
// env 组装末尾合并，让 mise 装出的工具在 shims 不在 PATH 时也能被启动链解析。
// 解析失败静默回退 PATH 直解（1.0–1.6 行为不变）。
// ---------------------------------------------------------------------------

fn kind_primary_tool(kind: &str) -> Option<crate::toolchain::ToolKind> {
    match kind {
        "spring-boot" => Some(crate::toolchain::ToolKind::Java),
        "node" => Some(crate::toolchain::ToolKind::Node),
        "python" => Some(crate::toolchain::ToolKind::Python),
        "go" => Some(crate::toolchain::ToolKind::Go),
        _ => None,
    }
}

/// 仅当 spec 显式 `toolchain.manager: mise` 时生效：`mise which <tool>` 解析
/// 当前 kind 的主工具并合并 PATH env_delta。失败静默（PATH 兜底）。
pub fn apply_pinned_mise_env(
    toolchain: Option<&crate::spec::ToolchainSpec>,
    kind: &str,
    root: &Path,
    env: &mut IndexMap<String, String>,
) {
    use crate::toolchain::{resolver, runner::ProcessRunner, ProviderKind};
    let Some(tc) = toolchain else { return };
    if tc.manager != Some(crate::spec::ToolchainManager::Mise) {
        return;
    }
    let Some(tool) = kind_primary_tool(kind) else {
        return;
    };
    let runner = ProcessRunner;
    if let Ok(resolved) = resolver::resolve_tool(&runner, ProviderKind::Mise, tool, root) {
        for (k, v) in &resolved.env_delta {
            env.insert(k.clone(), v.clone());
        }
    }
}

/// 1.2 §11.2：`mvn [-pl module] package -DskipTests` + build_args（不默认 -am）。
pub fn plan_jar_build(svc: &ServiceSpec, env: IndexMap<String, String>) -> Result<CommandSpec> {
    plan_jar_build_in(svc, env, None)
}

/// 带 root 的 jar 构建规划：缺省 build_tool 时按构建文件探测（§5.1/§5.3）。
pub fn plan_jar_build_in(
    svc: &ServiceSpec,
    env: IndexMap<String, String>,
    root: Option<&Path>,
) -> Result<CommandSpec> {
    let bt = match root {
        Some(r) => resolve_build_tool(r, svc)?,
        None => explicit_build_tool(svc).unwrap_or(BuildTool::Maven),
    };
    let module = svc.module.as_deref().unwrap_or(".");
    match bt {
        BuildTool::Maven => {
            let mut args = if module == "." {
                vec!["package".into()]
            } else {
                vec!["-pl".into(), module.into(), "package".into()]
            };
            args.push("-DskipTests".into());
            args.extend(svc.build_args.iter().cloned());
            Ok(CommandSpec {
                program: "mvn.cmd".into(),
                args,
                cwd_rel: svc.cwd.clone().unwrap_or_else(|| ".".into()),
                env,
            })
        }
        BuildTool::Gradle => {
            // 1.4 §5.3：`gradlew [:module:]bootJar`；bootJar 默认不跑测试，
            // 不追加 -DskipTests 等价物，用户需要时写 build_args。
            let mut args = gradle_task_args(module, "bootJar");
            args.extend(svc.build_args.iter().cloned());
            Ok(CommandSpec {
                program: gradle_wrapper_file_name().into(),
                args,
                cwd_rel: svc.cwd.clone().unwrap_or_else(|| ".".into()),
                env,
            })
        }
    }
}

/// 1.2 §11.2：`java -jar <artifact>` + extra_args；artifact 由 jar 编排插到 args[1]。
pub fn plan_jar_run(svc: &ServiceSpec, env: IndexMap<String, String>) -> CommandSpec {
    let mut args = vec!["-jar".into()];
    args.extend(svc.extra_args.iter().cloned());
    CommandSpec {
        program: "java.exe".into(),
        args,
        cwd_rel: svc.cwd.clone().unwrap_or_else(|| ".".into()),
        env,
    }
}

pub fn log_file_rel(kind: &str, id: &str) -> PathBuf {
    match kind {
        "script" => PathBuf::from(".supertask/logs/scripts").join(format!("{id}.log")),
        _ => PathBuf::from(".supertask/logs").join(format!("{id}.log")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use crate::spec::parse_yaml;
    use std::fs;

    // ---- 1.7：python / go / generic ----

    fn tmp_ws(name: &str) -> PathBuf {
        let base = std::env::temp_dir().join(format!("st-launcher-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(&base).unwrap();
        base
    }

    fn make_venv(dir: &Path, venv: &str) {
        let rel = if cfg!(windows) {
            "Scripts/python.exe"
        } else {
            "bin/python"
        };
        let p = dir.join(venv).join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(&p, b"").unwrap();
    }

    #[test]
    fn python_entry_mode_and_venv_priority() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: python
    dir: backend
    entry: main.py
    port: 8000
"#,
        )
        .unwrap();
        // 无 root（root=None）：PATH 兜底
        let c = plan_service(&f, "api").unwrap();
        assert_eq!(c.program, python_program_name());
        assert_eq!(c.args, ["main.py"]);
        assert_eq!(c.cwd_rel, "backend");
        assert_eq!(c.env.get("PORT").map(String::as_str), Some("8000"));
        // venv 优先：dir/.venv → 绝对路径（entry 文件需存在）
        let root = tmp_ws("py-venv");
        let dir = root.join("backend");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("main.py"), b"").unwrap();
        make_venv(&dir, ".venv");
        let c = plan_service_in(&f, "api", Some(&root)).unwrap();
        assert!(c.program.contains(".venv"), "program={}", c.program);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn python_module_mode_and_coexist_prefers_dot_venv() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: python
    dir: .
    module: uvicorn
    extra_args: ["app:app", "--port", "8000"]
"#,
        )
        .unwrap();
        let root = tmp_ws("py-module");
        make_venv(&root, ".venv");
        make_venv(&root, "venv");
        let c = plan_service_in(&f, "api", Some(&root)).unwrap();
        assert_eq!(c.args, ["-m", "uvicorn", "app:app", "--port", "8000"]);
        assert!(c.program.contains(".venv"), "program={}", c.program);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn python_entry_not_found_is_hard_error_at_launch() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: python
    dir: backend
    entry: missing.py
"#,
        )
        .unwrap();
        let root = tmp_ws("py-missing");
        let e = plan_service_in(&f, "api", Some(&root)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::EntryNotFound);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn go_defaults_and_package_normalization() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: go
    port: 8080
    extra_args: ["-conf", "dev.yaml"]
"#,
        )
        .unwrap();
        let c = plan_service(&f, "api").unwrap();
        assert_eq!(c.program, go_program_name());
        assert_eq!(c.args, ["run", ".", "-conf", "dev.yaml"]);
        assert_eq!(c.cwd_rel, ".");
        assert_eq!(c.env.get("PORT").map(String::as_str), Some("8080"));
        // 裸路径归一为 ./
        let (f2, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: go
    package: cmd/server
"#,
        )
        .unwrap();
        let c = plan_service(&f2, "api").unwrap();
        assert_eq!(c.args, ["run", "./cmd/server"]);
        // 存在性检查：目录存在通过
        let root = tmp_ws("go-ok");
        fs::create_dir_all(root.join("cmd/server")).unwrap();
        assert!(plan_service_in(&f2, "api", Some(&root)).is_ok());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn go_package_not_found() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: go
    dir: srv
    package: ./cmd/missing
"#,
        )
        .unwrap();
        let root = tmp_ws("go-missing");
        let e = plan_service_in(&f, "api", Some(&root)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::PackageNotFound);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn generic_path_name_and_relative_program() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  worker:
    kind: generic
    program: deno
    args: ["run", "--allow-net", "main.ts"]
    port: 4800
  tool:
    kind: generic
    program: bin/tool.exe
"#,
        )
        .unwrap();
        let c = plan_service(&f, "worker").unwrap();
        assert_eq!(c.program, "deno");
        assert_eq!(c.args, ["run", "--allow-net", "main.ts"]);
        assert_eq!(c.cwd_rel, ".");
        // generic 不注入 PORT
        assert!(!c.env.contains_key("PORT"));
        // 相对路径 program：规划期解析为绝对路径（存在性检查）
        let root = tmp_ws("generic-rel");
        fs::create_dir_all(root.join("bin")).unwrap();
        fs::write(root.join("bin/tool.exe"), b"").unwrap();
        let c = plan_service_in(&f, "tool", Some(&root)).unwrap();
        assert!(Path::new(&c.program).is_absolute());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn generic_missing_program_file() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  tool:
    kind: generic
    program: bin/absent.exe
"#,
        )
        .unwrap();
        let root = tmp_ws("generic-missing");
        let e = plan_service_in(&f, "tool", Some(&root)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::MissingTool);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn v17_validation_matrix() {
        let cases: [(&str, bool); 9] = [
            // python：dir 必填
            ("python-no-dir", false),
            // python：entry XOR module
            ("python-both", false),
            ("python-neither", false),
            // python 合法
            ("python-ok", true),
            // go 合法；外来字段拒绝
            ("go-ok", true),
            ("go-entry", false),
            // generic：program 必填；args 合法
            ("generic-no-program", false),
            ("generic-ok", true),
            // generic program 含 .. 拒绝
            ("generic-escape", false),
        ];
        let yamls: [(&str, &str); 9] = [
            ("python-no-dir", "version: 1\nservices:\n  s:\n    kind: python\n    entry: main.py\n"),
            ("python-both", "version: 1\nservices:\n  s:\n    kind: python\n    dir: .\n    entry: main.py\n    module: app\n"),
            ("python-neither", "version: 1\nservices:\n  s:\n    kind: python\n    dir: .\n"),
            ("python-ok", "version: 1\nservices:\n  s:\n    kind: python\n    dir: backend\n    entry: main.py\n    port: 8000\n"),
            ("go-ok", "version: 1\nservices:\n  s:\n    kind: go\n    package: ./cmd/x\n    port: 8080\n"),
            ("go-entry", "version: 1\nservices:\n  s:\n    kind: go\n    entry: main.py\n"),
            ("generic-no-program", "version: 1\nservices:\n  s:\n    kind: generic\n    args: [a]\n"),
            ("generic-ok", "version: 1\nservices:\n  s:\n    kind: generic\n    program: deno\n    args: [run, main.ts]\n    port: 4800\n"),
            ("generic-escape", "version: 1\nservices:\n  s:\n    kind: generic\n    program: ../evil.exe\n"),
        ];
        for (id, ok) in cases {
            let yaml = yamls.iter().find(|(k, _)| *k == id).unwrap().1;
            let r = parse_yaml(yaml);
            assert_eq!(
                r.is_ok(),
                ok,
                "case {id}: {:?}",
                r.err().map(|e| e.message().to_string())
            );
        }
    }

    #[test]
    fn v17_defaults_apply() {
        let (mut f, _) = parse_yaml(
            r#"
version: 1
services:
  py:
    kind: python
    dir: .
    entry: main.py
  g:
    kind: go
  gen:
    kind: generic
    program: x
  genport:
    kind: generic
    program: x
    port: 1234
"#,
        )
        .unwrap();
        f.apply_defaults();
        assert_eq!(f.services["py"].grace_secs, Some(15));
        assert_eq!(f.services["g"].grace_secs, Some(60));
        assert_eq!(f.services["gen"].grace_secs, Some(15));
        // 无 port 的 generic：health 缺省 none（不设 tcp）
        assert!(f.services["gen"].health.is_none());
        // 有 port：默认 tcp
        assert!(f.services["genport"].health.is_some());
    }

    #[test]
    fn spring_argv() {
        let (f, _) = parse_yaml(
            r#"
version: 1
env:
  SPRING_PROFILES_ACTIVE: local
services:
  api:
    kind: spring-boot
    module: user-service
    port: 8081
    extra_args: ["-DskipTests"]
"#,
        )
        .unwrap();
        let c = plan_service(&f, "api").unwrap();
        assert_eq!(c.program, "mvn.cmd");
        assert_eq!(
            c.args,
            vec!["-pl", "user-service", "spring-boot:run", "-DskipTests"]
        );
        assert_eq!(c.env.get("SERVER_PORT").unwrap(), "8081");
        assert_eq!(c.env.get("SPRING_PROFILES_ACTIVE").unwrap(), "local");
    }

    #[test]
    fn spring_single_module_omits_pl() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  app:
    kind: spring-boot
    module: "."
    port: 8080
"#,
        )
        .unwrap();
        let c = plan_service(&f, "app").unwrap();
        assert_eq!(c.args, vec!["spring-boot:run"]);
    }

    #[test]
    fn compose_kind_not_planned_as_local_command() {
        let (f, w) = parse_yaml(
            r#"
version: 1
services:
  db:
    kind: compose
    service: db
    extra: true
  api:
    kind: spring-boot
    module: api
    port: 8080
docker: {}
"#,
        )
        .unwrap();
        // 1.3 phase 3 起 compose 是可启动 kind：解析无警告，启动走 engine 的
        // 容器运行时分支（spawn_compose），不再进入本地命令规划。
        assert!(!w.iter().any(|x| x.code == ErrorCode::KindUnsupported));
        assert!(f.docker.is_some());
        // plan_service 只负责 spring-boot/node 的本地 argv；对 compose 返回
        // KIND_UNSUPPORTED 作为「不该走这条路径」的兜底守卫。
        let e = plan_service(&f, "db").unwrap_err();
        assert_eq!(e.code(), ErrorCode::KindUnsupported);
        // 本地服务规划不受影响
        assert!(plan_service(&f, "api").is_ok());
    }

    #[test]
    fn jar_launch_parses_but_cannot_start() {
        let (f, _) = parse_yaml(
            r#"
version: 1
services:
  api:
    kind: spring-boot
    module: api
    port: 8080
    launch: jar
"#,
        )
        .unwrap();
        let e = plan_service(&f, "api").unwrap_err();
        assert_eq!(e.code(), ErrorCode::LaunchUnsupported);
    }

    // ---- 1.4 §5：Gradle 多模块 ----

    fn gradle_yaml(svc_body: &str) -> crate::spec::SuperTaskFile {
        parse_yaml(&format!(
            "version: 1\nservices:\n  api:\n    kind: spring-boot\n{svc_body}\n"
        ))
        .unwrap()
        .0
    }

    #[test]
    fn gradle_bootrun_argv() {
        let f = gradle_yaml(
            "    build_tool: gradle\n    module: user-service\n    port: 8081\n    extra_args: [\"--offline\"]",
        );
        let c = plan_service(&f, "api").unwrap();
        #[cfg(windows)]
        assert_eq!(c.program, "gradlew.bat");
        #[cfg(not(windows))]
        assert_eq!(c.program, "gradlew");
        assert_eq!(c.args, vec![":user-service:bootRun", "--offline"]);
        assert_eq!(c.env.get("SERVER_PORT").unwrap(), "8081");
        assert_eq!(c.cwd_rel, ".");
    }

    #[test]
    fn gradle_single_module_omits_task_prefix() {
        let f = gradle_yaml("    build_tool: gradle\n    module: \".\"\n    port: 8080");
        let c = plan_service(&f, "api").unwrap();
        assert_eq!(c.args, vec!["bootRun"]);
    }

    #[test]
    fn gradle_nested_module_uses_colon_path() {
        let f = gradle_yaml("    build_tool: gradle\n    module: apps/api\n    port: 8080");
        let c = plan_service(&f, "api").unwrap();
        assert_eq!(c.args, vec![":apps:api:bootRun"]);
    }

    #[test]
    fn gradle_bootjar_argv_no_skip_tests() {
        let f = gradle_yaml(
            "    build_tool: gradle\n    module: user-service\n    port: 8080\n    launch: jar\n    build_args: [\"--info\"]",
        );
        let c = plan_jar_build_in(&f.services["api"], Default::default(), None).unwrap();
        #[cfg(windows)]
        assert_eq!(c.program, "gradlew.bat");
        // §5.3：默认不追加 -DskipTests 等价物；build_args 按 argv 追加
        assert_eq!(c.args, vec![":user-service:bootJar", "--info"]);
        assert!(!c.args.iter().any(|a| a.contains("skipTests")));
    }

    #[test]
    fn gradle_jar_run_is_still_java() {
        let f =
            gradle_yaml("    build_tool: gradle\n    module: m\n    port: 8080\n    launch: jar");
        let c = plan_jar_run(&f.services["api"], Default::default());
        assert_eq!(c.program, "java.exe");
        assert_eq!(c.args, vec!["-jar"]);
    }

    #[test]
    fn detect_build_tool_rules() {
        let root = std::env::temp_dir().join(format!("st-bt-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("g")).unwrap();
        fs::create_dir_all(root.join("m")).unwrap();
        fs::create_dir_all(root.join("both")).unwrap();
        fs::create_dir_all(root.join("none")).unwrap();
        fs::write(root.join("g/build.gradle"), "").unwrap();
        fs::write(root.join("g/build.gradle.kts"), "").unwrap();
        fs::write(root.join("m/pom.xml"), "").unwrap();
        fs::write(root.join("both/pom.xml"), "").unwrap();
        fs::write(root.join("both/build.gradle"), "").unwrap();
        assert_eq!(
            detect_build_tool(&root.join("g")).unwrap(),
            BuildTool::Gradle
        );
        assert_eq!(
            detect_build_tool(&root.join("m")).unwrap(),
            BuildTool::Maven
        );
        // §5.1：两者并存 → BUILD_TOOL_AMBIGUOUS；都没有 → 工具缺失
        assert_eq!(
            detect_build_tool(&root.join("both")).unwrap_err().code(),
            ErrorCode::BuildToolAmbiguous
        );
        assert_eq!(
            detect_build_tool(&root.join("none")).unwrap_err().code(),
            ErrorCode::MissingTool
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn ambiguous_and_missing_are_hard_errors_at_plan_time() {
        let root = std::env::temp_dir().join(format!("st-bt-plan-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("mod")).unwrap();
        fs::write(root.join("mod/pom.xml"), "").unwrap();
        fs::write(root.join("mod/build.gradle"), "").unwrap();
        // 并存且未显式指定 → 启动前硬错误
        let f = gradle_yaml("    module: mod\n    port: 8080");
        let e = plan_service_in(&f, "api", Some(&root)).unwrap_err();
        assert_eq!(e.code(), ErrorCode::BuildToolAmbiguous);
        // 目录里什么都没有 → 按工具缺失处理
        fs::create_dir_all(root.join("empty")).unwrap();
        let f2 = gradle_yaml("    module: empty\n    port: 8080");
        let e2 = plan_service_in(&f2, "api", Some(&root)).unwrap_err();
        assert_eq!(e2.code(), ErrorCode::MissingTool);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn explicit_build_tool_skips_detection() {
        let root = std::env::temp_dir().join(format!("st-bt-explicit-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("pom.xml"), "").unwrap();
        fs::write(root.join("build.gradle"), "").unwrap();
        // 显式 build_tool 跳过探测（§5.1）：并存文件也不报错
        let f = gradle_yaml("    build_tool: gradle\n    module: \".\"\n    port: 8080");
        let c = plan_service_in(&f, "api", Some(&root)).unwrap();
        assert_eq!(c.args, vec!["bootRun"]);
        let f2 = gradle_yaml("    build_tool: maven\n    module: \".\"\n    port: 8080");
        let c2 = plan_service_in(&f2, "api", Some(&root)).unwrap();
        assert_eq!(c2.program, "mvn.cmd");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wrapper_preferred_then_path_gradle() {
        let root = std::env::temp_dir().join(format!("st-bt-wrapper-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        #[cfg(windows)]
        let wrapper = root.join("gradlew.bat");
        #[cfg(not(windows))]
        let wrapper = root.join("gradlew");
        fs::write(&wrapper, "@echo off").unwrap();
        let (prog, args, warns) =
            resolve_gradle_launcher(&root, "mod", "gradlew.bat", &[":mod:bootRun".into()]).unwrap();
        assert_eq!(prog, wrapper.display().to_string());
        assert_eq!(args, vec![":mod:bootRun"]);
        assert!(warns.is_empty());

        // module 目录的 wrapper 也能找到
        fs::remove_file(&wrapper).unwrap();
        fs::create_dir_all(root.join("mod")).unwrap();
        #[cfg(windows)]
        fs::write(root.join("mod/gradlew.bat"), "@echo off").unwrap();
        #[cfg(not(windows))]
        fs::write(root.join("mod/gradlew"), "#!/bin/sh").unwrap();
        let (prog, _, _) =
            resolve_gradle_launcher(&root, "mod", "gradlew.bat", &[":mod:bootRun".into()]).unwrap();
        assert_eq!(
            prog,
            root.join("mod")
                .join(gradle_wrapper_file_name())
                .display()
                .to_string()
        );
        let _ = fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn wrapper_without_exec_bit_runs_via_sh_once() {
        use std::os::unix::fs::PermissionsExt;
        let root = std::env::temp_dir().join(format!("st-bt-sh-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let wrapper = root.join("gradlew");
        fs::write(&wrapper, "#!/bin/sh").unwrap();
        fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o644)).unwrap();
        let (prog, args, warns) =
            resolve_gradle_launcher(&root, ".", "gradlew", &[":m:bootRun".into()]).unwrap();
        assert_eq!(prog, "sh");
        assert_eq!(args[0], wrapper.display().to_string());
        assert_eq!(args[1], ":m:bootRun");
        assert_eq!(warns.len(), 1);
        // 警告一次：第二次不再带警告，但仍走 sh
        let (prog2, _, warns2) =
            resolve_gradle_launcher(&root, ".", "gradlew", &[":m:bootRun".into()]).unwrap();
        assert_eq!(prog2, "sh");
        assert!(warns2.is_empty());
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn no_wrapper_no_path_gradle_is_gradle_wrapper_missing() {
        if crate::probe::find_on_path("gradle").is_some() {
            eprintln!("skip: PATH 中存在 gradle，无法模拟双缺失");
            return;
        }
        let root = std::env::temp_dir().join(format!("st-bt-missing-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let e = resolve_gradle_launcher(&root, ".", "gradlew.bat", &[]).unwrap_err();
        assert_eq!(e.code(), ErrorCode::GradleWrapperMissing);
        // details 携带 `gradle wrapper` 建议
        assert!(e.to_string().contains("gradle wrapper"));
        let _ = fs::remove_dir_all(&root);
    }
}
