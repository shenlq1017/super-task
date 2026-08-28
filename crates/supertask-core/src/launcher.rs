use std::path::{Path, PathBuf};

use indexmap::IndexMap;

use crate::error::{Error, ErrorCode, Result};
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
            "node" if !env.contains_key("PORT") => {
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
    use std::fs;
    use crate::error::ErrorCode;
    use crate::spec::parse_yaml;

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
        let f = gradle_yaml("    build_tool: gradle\n    module: m\n    port: 8080\n    launch: jar");
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
        assert_eq!(detect_build_tool(&root.join("g")).unwrap(), BuildTool::Gradle);
        assert_eq!(detect_build_tool(&root.join("m")).unwrap(), BuildTool::Maven);
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
            resolve_gradle_launcher(&root, "mod", "gradlew.bat", &[":mod:bootRun".into()])
                .unwrap();
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
