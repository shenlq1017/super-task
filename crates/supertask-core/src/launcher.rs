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
    }
    )
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
    fn compose_kind_cannot_start() {
        let (f, w) = parse_yaml(
            r#"
version: 1
services:
  db:
    kind: compose
    extra: true
  api:
    kind: spring-boot
    module: api
    port: 8080
docker: {}
"#,
        )
        .unwrap();
        assert!(w.iter().any(|x| x.code == ErrorCode::KindUnsupported));
        assert!(f.docker.is_some());
        let e = plan_service(&f, "db").unwrap_err();
        assert_eq!(e.code(), ErrorCode::KindUnsupported);
    }
}
