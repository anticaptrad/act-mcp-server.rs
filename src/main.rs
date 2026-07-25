//! act-mcp-server — Model Context Protocol server for the AntiCapTrad platform.
//!
//! Exposes an HTTP JSON-RPC MCP endpoint plus k8s health/readiness probes.
//! Deployed to the cluster at ~/codes/ores/k8s-cluster.

mod auth;
mod config;
mod mcp;
mod routes;
mod state;
mod telemetry;

use std::net::SocketAddr;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cfg = config::Config::from_env();
    telemetry::init(&cfg.service_name)?;

    if cfg.auth_secret.is_none() {
        tracing::warn!("SERVER_AUTH_SECRET not set; /mcp will reject every request");
    }

    let app = routes::router(state::AppState {
        auth_secret: cfg.auth_secret.clone(),
    });

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(%addr, service = %cfg.service_name, "act-mcp-server listening");

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutdown complete");
    telemetry::shutdown();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl-C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
