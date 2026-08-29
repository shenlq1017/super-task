use std::net::SocketAddr;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub database_url: String,
    pub seed: bool,
    pub seed_email: String,
    pub seed_password: Option<String>,
    pub entities_max: u64,
    pub bytes_max: u64,
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
        Ok(Self {
            bind,
            database_url,
            seed,
            seed_email,
            seed_password,
            entities_max: 100,
            bytes_max: 10_000_000,
        })
    }
}
