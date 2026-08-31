use std::net::SocketAddr;

use supertask_cloud_server::{admin, app, auth, config::Config, state::AppState};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "supertask_cloud_server=info".into()),
        )
        .init();
    let config = Config::from_env().map_err(std::io::Error::other)?;
    if config.bind.ip().is_loopback() {
        tracing::warn!(bind = %config.bind, "cloud server is listening on loopback only");
    } else {
        tracing::warn!(
            bind = %config.bind,
            "cloud server is exposed beyond loopback; the admin console is exposed with it, \
             put both behind HTTPS and access control"
        );
    }
    if config.seed {
        tracing::warn!(email = %config.seed_email, "development seed account is enabled");
    }
    let state = AppState::connect(config.clone()).await?;
    if config.seed {
        let password = config
            .seed_password
            .as_deref()
            .ok_or_else(|| std::io::Error::other("seed password missing"))?;
        auth::seed_account(&state.pool, &config.seed_email, password).await?;
    }
    match (config.admin_email.as_deref(), config.admin_password.as_deref()) {
        (Some(email), Some(password)) => {
            admin::bootstrap_admin(&state.pool, email, password).await?;
            tracing::warn!(
                email,
                address = %config.bind,
                "cloud admin console is enabled; manage accounts at /admin/"
            );
        }
        _ => {
            tracing::info!(
                "admin console API is unavailable until SUPERTASK_ADMIN_EMAIL and \
                 SUPERTASK_ADMIN_PASSWORD are both set"
            );
        }
    }
    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    tracing::info!(address = %listener.local_addr()?, "cloud server listening");
    axum::serve(listener, app(state))
        .with_graceful_shutdown(shutdown_signal())
        .await?;
    Ok(())
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{signal, SignalKind};
        let mut term = signal(SignalKind::terminate()).expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {},
            _ = term.recv() => {},
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

#[allow(dead_code)]
fn _socket_addr(_: SocketAddr) {}
