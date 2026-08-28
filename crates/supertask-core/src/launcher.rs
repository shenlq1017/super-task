use std::path::PathBuf;

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

/// Build argv from spec only. Never takes cmdline from IPC.
pub fn plan_service(file: &SuperTaskFile, id: &str) -> Result<CommandSpec> {
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
        "spring-boot" => plan_spring(svc, env),
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

fn plan_spring(svc: &ServiceSpec, env: IndexMap<String, String>) -> Result<CommandSpec> {
    let launch = svc.launch.as_deref().unwrap_or("run");
    if launch != "run" {
        return Err(Error::new(
            ErrorCode::LaunchUnsupported,
            format!("launch '{launch}' 本版尚未实现启动"),
        ));
    }
    let module = svc.module.as_deref().unwrap();
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
    let module = svc.module.as_deref().unwrap_or(".");
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
}
