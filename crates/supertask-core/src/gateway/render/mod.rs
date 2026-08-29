//! 1.6 配置渲染（规格 §5）：IR → nginx.conf / Caddyfile / httpd.conf。
//!
//! 纯函数：无 IO、无平台分支；平台差异（apache 模块集/路径）经入参注入。
//! 输出只含白名单指令集，无用户原文注入点；golden 测试锁定字节级输出。

mod apache;
mod caddy;
mod nginx;

pub use apache::{render_apache, ApacheOptions, APACHE_MODULES_UNIX, APACHE_MODULES_WINDOWS};
pub use caddy::render_caddy;
pub use nginx::render_nginx;

/// 路径文本约定：正斜杠、无尾随分隔符（nginx/caddy/apache 均可接受）。
pub fn posix(p: &str) -> String {
    p.replace('\\', "/")
}

/// 组内 location 按最长前缀优先（nginx 语义上无关紧要，apache ProxyPass
/// 先匹配先生效、caddy handle 按声明序，统一最长优先最稳）。
pub(crate) fn sorted_locations(
    locations: &[crate::gateway::model::GatewayLocation],
) -> Vec<&crate::gateway::model::GatewayLocation> {
    let mut out: Vec<&crate::gateway::model::GatewayLocation> = locations.iter().collect();
    out.sort_by(|a, b| b.path.len().cmp(&a.path.len()).then(a.path.cmp(&b.path)));
    out
}

/// 组内是否已有根路由（有则该组自带 catch-all，无需 404 兜底）。
pub(crate) fn has_root(locations: &[crate::gateway::model::GatewayLocation]) -> bool {
    locations.iter().any(|l| l.path == "/")
}

/// 统一渲染入口：按 IR 的 kind 输出（产物文件名, 内容）。
/// `apache_modules_dir` 仅 apache 用到（其余引擎忽略）。
pub fn render_conf(
    ir: &crate::gateway::model::ResolvedGateway,
    dir: &str,
    apache_modules_dir: &str,
) -> crate::error::Result<(&'static str, String)> {
    use crate::spec::GatewayKind;
    Ok(match ir.kind {
        GatewayKind::Nginx => ("nginx.conf", render_nginx(ir, dir)),
        GatewayKind::Caddy => ("Caddyfile", render_caddy(ir)),
        GatewayKind::Apache => (
            "httpd.conf",
            render_apache(
                ir,
                &ApacheOptions {
                    dir: dir.to_string(),
                    modules_dir: apache_modules_dir.to_string(),
                    modules: apache_modules(),
                },
            ),
        ),
    })
}

/// 当前平台的 apache 模块集（渲染入参；探测/校验不内置路径）。
pub fn apache_modules() -> &'static [(&'static str, &'static str)] {
    #[cfg(windows)]
    {
        APACHE_MODULES_WINDOWS
    }
    #[cfg(not(windows))]
    {
        APACHE_MODULES_UNIX
    }
}
