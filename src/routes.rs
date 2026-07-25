//! HTTP surface: k8s probes plus the MCP JSON-RPC endpoint.

use axum::{Json, Router, middleware, routing::get, routing::post};
use serde_json::{Value, json};

use crate::auth;
use crate::mcp;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    // The MCP surface is tool execution, so it sits behind the shared secret.
    // Probes stay public — the kubelet sends no headers.
    let protected = Router::new()
        .route("/mcp", post(mcp::handle))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_server_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .merge(protected)
        .with_state(state)
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready() -> Json<Value> {
    Json(json!({ "ready": true }))
}
