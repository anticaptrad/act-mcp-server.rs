//! act-mcp-server — Model Context Protocol server for the AntiCapTrad platform.
//!
//! Exposes an HTTP JSON-RPC MCP endpoint plus k8s health/readiness probes.
//! Deployed to the cluster at ~/codes/ores/k8s-cluster.

#[path = "../generated/rust/env.rs"]
mod env;
#[path = "../generated/rust/runtime.rs"]
mod env_runtime;

mod auth;
mod config;
mod mcp;
mod routes;
mod startup;
mod state;
mod telemetry;

use std::net::SocketAddr;
use tokio::signal;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let startup =
        startup::process_startup_flags().map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let cfg = config::Config::from_env_with_port(Some(startup.port))?;
    telemetry::init(&cfg.service_name, startup.log_filter)?;

    if cfg.auth_secret.is_none() {
        tracing::warn!("SERVER_AUTH_SECRET not set; /mcp will reject every request");
    }

    let app = routes::router(state::AppState {
        auth_secret: cfg.auth_secret.clone(),
        allowed_hosts: cfg.allowed_hosts.clone(),
        allowed_origins: cfg.allowed_origins.clone(),
    });

    let addr = SocketAddr::from(([0, 0, 0, 0], cfg.port));
    let listener = tokio::net::TcpListener::bind(addr).await?;
    tracing::info!(
        %addr,
        service = %cfg.service_name,
        allowed_host_count = cfg.allowed_hosts.len(),
        allowed_origin_count = cfg.allowed_origins.len(),
        "act-mcp-server listening"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    tracing::info!("shutdown complete");
    telemetry::shutdown();
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        if let Err(error) = signal::ctrl_c().await {
            tracing::error!(%error, "failed to install or receive Ctrl-C signal");
        }
    };

    #[cfg(unix)]
    let terminate = async {
        match signal::unix::signal(signal::unix::SignalKind::terminate()) {
            Ok(mut signal) => {
                signal.recv().await;
            }
            Err(error) => {
                tracing::error!(%error, "failed to install SIGTERM handler");
                std::future::pending::<()>().await;
            }
        }
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    tracing::info!("shutdown signal received");
}
