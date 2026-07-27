//! Minimal Model Context Protocol (MCP) endpoint over JSON-RPC 2.0.
//!
//! Implements the handshake and tool primitives a client needs to discover and
//! invoke tools: `initialize`, `ping`, `tools/list`, and `tools/call`. Transport
//! is HTTP POST (streamable-HTTP style) rather than stdio so the server can run
//! as a long-lived pod in the k8s cluster.

use axum::{Json, http::StatusCode, response::IntoResponse};
use serde::Deserialize;
use serde_json::{Value, json};

/// MCP protocol revision this server implements.
const PROTOCOL_VERSION: &str = "2024-11-05";

/// JSON-RPC standard error codes.
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    jsonrpc: Option<String>,
    method: String,
    #[serde(default)]
    params: Value,
    /// Absent for notifications; present for requests expecting a response.
    id: Option<Value>,
}

fn ok(id: Option<Value>, result: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "result": result })
}

fn err(id: Option<Value>, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

/// Static catalog of tools this server exposes.
fn tool_catalog() -> Value {
    json!([
        {
            "name": "ping",
            "description": "Health probe that echoes an optional message.",
            "inputSchema": {
                "type": "object",
                "properties": { "message": { "type": "string" } },
                "additionalProperties": false
            }
        }
    ])
}

pub async fn handle(Json(req): Json<JsonRpcRequest>) -> impl IntoResponse {
    let is_notification = req.id.is_none();
    let response = dispatch(&req);

    // Notifications must not receive a response body.
    if is_notification {
        return StatusCode::ACCEPTED.into_response();
    }
    Json(response).into_response()
}

fn dispatch(req: &JsonRpcRequest) -> Value {
    let id = req.id.clone();
    match req.method.as_str() {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": PROTOCOL_VERSION,
                "capabilities": { "tools": { "listChanged": false } },
                "serverInfo": { "name": "act-mcp-server", "version": env!("CARGO_PKG_VERSION") }
            }),
        ),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({ "tools": tool_catalog() })),
        "tools/call" => call_tool(id, &req.params),
        other => {
            tracing::debug!(method = other, "unknown MCP method");
            err(id, METHOD_NOT_FOUND, "method not found")
        }
    }
}

fn call_tool(id: Option<Value>, params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str);
    let arguments = params
        .get("arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));

    match name {
        Some("ping") => {
            let message = arguments
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("pong");
            ok(
                id,
                json!({
                    "content": [ { "type": "text", "text": message } ],
                    "isError": false
                }),
            )
        }
        Some(unknown) => err(id, INVALID_PARAMS, &format!("unknown tool: {unknown}")),
        None => err(id, INVALID_PARAMS, "missing tool name"),
    }
}
