//! HTTP surface: k8s probes plus the MCP JSON-RPC endpoint.

use axum::{Json, Router, routing::get, routing::post};
use serde_json::{Value, json};

use crate::mcp;

pub fn router() -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .route("/mcp", post(mcp::handle))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn ready() -> Json<Value> {
    Json(json!({ "ready": true }))
}
