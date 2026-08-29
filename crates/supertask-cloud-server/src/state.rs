use std::{path::Path, str::FromStr};

use sqlx::{
    sqlite::{SqliteConnectOptions, SqlitePoolOptions},
    SqlitePool,
};

use crate::{config::Config, error::AppError};

#[derive(Clone)]
pub struct AppState {
    pub pool: SqlitePool,
    pub config: Config,
}

impl AppState {
    pub async fn connect(config: Config) -> Result<Self, AppError> {
        let is_memory = config.database_url == ":memory:"
            || config.database_url == "sqlite::memory:"
            || config.database_url.starts_with("sqlite::memory:");
        let url = if config.database_url == ":memory:" {
            "sqlite::memory:".to_string()
        } else {
            config.database_url.clone()
        };
        if !is_memory {
            let raw_path = url.strip_prefix("sqlite://").unwrap_or(&url);
            if let Some(parent) = Path::new(raw_path).parent() {
                if !parent.as_os_str().is_empty() {
                    std::fs::create_dir_all(parent).map_err(|error| {
                        AppError::Internal(format!("无法创建数据库目录: {error}"))
                    })?;
                }
            }
        }
        let connect_options = SqliteConnectOptions::from_str(&url)
            .map_err(|error| AppError::Internal(format!("SQLite URL 无效: {error}")))?
            .create_if_missing(true);
        let mut options = SqlitePoolOptions::new();
        // Every pooled SQLite connection gets a distinct :memory: database. A
        // single connection is therefore required for tests and local use.
        options = options.max_connections(if is_memory { 1 } else { 8 });
        let pool = options.connect_with(connect_options).await?;
        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await?;
        sqlx::migrate!().run(&pool).await?;
        Ok(Self { pool, config })
    }
}
