use std::collections::HashSet;

use serde_yaml::Value;

use crate::error::{Error, ErrorCode, Result};
use crate::sandbox::is_loopback_url;

use super::file::{check_limits, HealthType, ParseWarning, SuperTaskFile};

pub fn validate(file: &SuperTaskFile) -> Result<Vec<ParseWarning>> {
    let mut warnings = Vec::new();

    if file.version != 1 {
        if file.version > 1 {
            warnings.push(ParseWarning {
                code: ErrorCode::SpecNewer,
                message: format!("version {} 新于本引擎，未知字段可能未执行", file.version),
            });
        } else {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("不支持 version {}", file.version),
            ));
        }
    }

    if file.root != "." {
        return Err(Error::new(
            ErrorCode::SpecInvalid,
            "root 只允许 \".\"",
        ));
    }

    if file.services.is_empty() {
        return Err(Error::new(ErrorCode::SpecInvalid, "至少需要一个 service"));
    }

    check_limits(file)?;

    let ids: HashSet<&str> = file.services.keys().map(|s| s.as_str()).collect();
    let mut ports: Vec<(String, u16)> = Vec::new();

    for (id, svc) in &file.services {
        match svc.kind.as_str() {
            "spring-boot" => {
                if svc.module.as_deref().unwrap_or("").is_empty() {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("{id}: spring-boot 需要 module"),
                    ));
                }
                if let Some(launch) = &svc.launch {
                    if launch != "run" {
                        return Err(Error::new(
                            ErrorCode::LaunchUnsupported,
                            format!("{id}: launch '{launch}' 本版仅支持 run"),
                        ));
                    }
                }
            }
            "node" => {
                if svc.dir.as_deref().unwrap_or("").is_empty() {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("{id}: node 需要 dir"),
                    ));
                }
            }
            _ => {
                warnings.push(ParseWarning {
                    code: ErrorCode::KindUnsupported,
                    message: format!("{id}: kind '{}' 本版不能启动，配置会保留", svc.kind),
                });
            }
        }

        for dep in &svc.depends_on {
            if !ids.contains(dep.as_str()) {
                return Err(Error::new(
                    ErrorCode::SpecInvalid,
                    format!("{id}: depends_on '{dep}' 不存在"),
                ));
            }
        }

        if let Some(port) = svc.port {
            if port == 0 {
                return Err(Error::new(ErrorCode::SpecInvalid, format!("{id}: 非法 port")));
            }
            if let Some((other, _)) = ports.iter().find(|(_, p)| *p == port) {
                warnings.push(ParseWarning {
                    code: ErrorCode::PortDup,
                    message: format!("端口 {port} 重复：{other} 与 {id}"),
                });
            }
            ports.push((id.clone(), port));
        }

        if let Some(h) = &svc.health {
            match h.r#type {
                HealthType::Tcp | HealthType::Http if svc.port.is_none() => {
                    return Err(Error::new(
                        ErrorCode::SpecInvalid,
                        format!("{id}: tcp/http 健康检查需要 port"),
                    ));
                }
                HealthType::Http => {
                    if let Some(url) = &h.http {
                        if !is_loopback_url(url) {
                            return Err(Error::new(
                                ErrorCode::HealthHostForbidden,
                                format!("{id}: 健康检查只允许 127.0.0.1/localhost"),
                            ));
                        }
                    }
                }
                _ => {}
            }
        }

        if let Some(dir) = &svc.dir {
            crate::sandbox::assert_rel_safe(dir)?;
        }
        if let Some(cwd) = &svc.cwd {
            crate::sandbox::assert_rel_safe(cwd)?;
        }
    }

    for (id, script) in &file.scripts {
        if script.cmds.iter().any(|c| c.trim().is_empty()) {
            return Err(Error::new(
                ErrorCode::SpecInvalid,
                format!("脚本 {id} 含空命令"),
            ));
        }
        if let Some(cwd) = &script.cwd {
            crate::sandbox::assert_rel_safe(cwd)?;
        }
    }

    if let Some(Value::Mapping(m)) = &file.secrets {
        if !m.is_empty() {
            warnings.push(ParseWarning {
                code: ErrorCode::FeatureSoon,
                message: "secrets 段 1.2 才会读取，本版忽略".into(),
            });
        }
    }

    Ok(warnings)
}
