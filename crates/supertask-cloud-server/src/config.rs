use std::{
    net::SocketAddr,
    path::PathBuf,
};

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub seed: bool,
    pub seed_email: String,
    pub seed_password: Option<String>,
    pub entities_max: u64,
    pub bytes_max: u64,
    pub admin_email: Option<String>,
    pub admin_password: Option<String>,
    pub console_dir: PathBuf,
}

impl Config {
    pub fn from_env() -> Result<Self, String> {
        let bind = std::env::var("SUPERTASK_BIND")
            .unwrap_or_else(|_| "127.0.0.1:8787".into())
            .parse()
            .map_err(|e| format!("SUPERTASK_BIND 无效: {e}"))?;
        let database_url = std::env::var("SUPERTASK_DATABASE_URL")
            .unwrap_or_else(|_| "sqlite://supertask-cloud.db".into());
        let seed = std::env::var("SUPERTASK_DEV_SEED")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let seed_email =
            std::env::var("SUPERTASK_SEED_EMAIL").unwrap_or_else(|_| "demo@supertask.local".into());
        let seed_password = std::env::var("SUPERTASK_SEED_PASSWORD").ok();
        if seed && seed_password.as_deref().unwrap_or("").is_empty() {
            return Err("启用 SUPERTASK_DEV_SEED 时必须设置 SUPERTASK_SEED_PASSWORD".into());
        }
        let admin_email = non_empty_env("SUPERTASK_ADMIN_EMAIL").map(|v| v.to_ascii_lowercase());
        let admin_password = non_empty_env("SUPERTASK_ADMIN_PASSWORD");
        if admin_email.is_some() != admin_password.is_some() {
            return Err("管理控制台需要同时设置 SUPERTASK_ADMIN_EMAIL 与 SUPERTASK_ADMIN_PASSWORD".into());
        }
        Ok(Self {
            bind,
            database_url,
            seed,
            seed_email,
            seed_password,
            entities_max: 100,
            bytes_max: 10_000_000,
            admin_email,
            admin_password,
            console_dir: non_empty_env("SUPERTASK_CONSOLE_DIR")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("cloud-console/dist")),
        })
    }

    pub fn admin_configured(&self) -> bool {
        self.admin_email.is_some() && self.admin_password.is_some()
    }

    pub fn console_ready(&self) -> bool {
        self.console_dir.join("index.html").is_file()
    }
}

fn non_empty_env(key: &str) -> Option<String> {
    std::env::var(key)
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
