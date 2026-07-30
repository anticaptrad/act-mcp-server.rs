//! Bounded Model Context Protocol endpoint over JSON-RPC 2.0 and HTTP POST.
//!
//! The server implements the handshake and tool primitives required for a
//! Streamable HTTP MCP client: `initialize`, `ping`, `tools/list`, and
//! `tools/call`. Notifications are accepted without a response body, request
//! identifiers are restricted to JSON strings or integers, and the optional
//! `MCP-Protocol-Version` header is validated after initialization.

use axum::{
    Json,
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Deserializer};
use serde_json::{Value, json};

/// Newest protocol revision this server implements.
const PROTOCOL_VERSION: &str = "2025-11-25";
const SUPPORTED_PROTOCOL_VERSIONS: &[&str] =
    &["2025-03-26", "2025-06-18", PROTOCOL_VERSION];
const PROTOCOL_VERSION_HEADER: &str = "mcp-protocol-version";
pub(crate) const MAX_REQUEST_BYTES: usize = 64 * 1024;
const MAX_PING_MESSAGE_BYTES: usize = 1_024;

/// JSON-RPC standard error codes.
const INVALID_REQUEST: i64 = -32600;
const METHOD_NOT_FOUND: i64 = -32601;
const INVALID_PARAMS: i64 = -32602;

#[derive(Debug)]
enum RpcId {
    Missing,
    Valid(Value),
    Invalid,
}

impl Default for RpcId {
    fn default() -> Self {
        Self::Missing
    }
}

impl RpcId {
    fn error_id(&self) -> Value {
        match self {
            Self::Valid(value) => value.clone(),
            Self::Missing | Self::Invalid => Value::Null,
        }
    }
}

impl<'de> Deserialize<'de> for RpcId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = Value::deserialize(deserializer)?;
        let valid = value.is_string()
            || value
                .as_number()
                .is_some_and(|number| number.as_i64().is_some() || number.as_u64().is_some());
        Ok(if valid {
            Self::Valid(value)
        } else {
            Self::Invalid
        })
    }
}

#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[serde(default)]
    jsonrpc: Option<String>,
    method: String,
    #[serde(default)]
    params: Value,
    /// Missing for notifications; explicit null and non-string/non-integer IDs
    /// are invalid requests rather than notifications.
    #[serde(default)]
    id: RpcId,
}

fn ok(id: Value, result: Value) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "result": result})
}

fn err(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {"code": code, "message": message}
    })
}

fn rpc_response(status: StatusCode, payload: Value) -> Response {
    (status, Json(payload)).into_response()
}

/// Static catalog of tools this server exposes.
fn tool_catalog() -> Value {
    json!([
        {
            "name": "ping",
            "title": "Ping ACT MCP",
            "description": "Health probe that echoes an optional bounded message without mutating platform state.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "maxLength": MAX_PING_MESSAGE_BYTES
                    }
                },
                "additionalProperties": false
            },
            "annotations": {
                "readOnlyHint": true,
                "destructiveHint": false,
                "idempotentHint": true,
                "openWorldHint": false
            }
        }
    ])
}

pub async fn handle(headers: HeaderMap, Json(req): Json<JsonRpcRequest>) -> Response {
    if req.jsonrpc.as_deref() != Some("2.0") {
        return rpc_response(
            StatusCode::BAD_REQUEST,
            err(req.id.error_id(), INVALID_REQUEST, "invalid JSON-RPC version"),
        );
    }
    if !protocol_header_supported(&headers) {
        return rpc_response(
            StatusCode::BAD_REQUEST,
            err(
                req.id.error_id(),
                INVALID_REQUEST,
                "unsupported MCP protocol version",
            ),
        );
    }

    let id = match req.id {
        RpcId::Missing => return StatusCode::ACCEPTED.into_response(),
        RpcId::Invalid => {
            return rpc_response(
                StatusCode::BAD_REQUEST,
                err(
                    Value::Null,
                    INVALID_REQUEST,
                    "request id must be a string or integer",
                ),
            );
        }
        RpcId::Valid(id) => id,
    };

    rpc_response(StatusCode::OK, dispatch(id, &req.method, &req.params))
}

fn dispatch(id: Value, method: &str, params: &Value) -> Value {
    match method {
        "initialize" => ok(
            id,
            json!({
                "protocolVersion": negotiated_protocol_version(params),
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {
                    "name": "act-mcp-server",
                    "title": "AntiCapTrad MCP Server",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "instructions": "Use the bounded, read-only ACT tool surface. Browser callers must use an explicitly allowlisted Origin."
            }),
        ),
        "ping" => ok(id, json!({})),
        "tools/list" => ok(id, json!({"tools": tool_catalog()})),
        "tools/call" => call_tool(id, params),
        other => {
            tracing::debug!(method = other, "unknown MCP method");
            err(id, METHOD_NOT_FOUND, "method not found")
        }
    }
}

fn call_tool(id: Value, params: &Value) -> Value {
    let Some(object) = params.as_object() else {
        return err(id, INVALID_PARAMS, "tools/call params must be an object");
    };
    let Some(name) = object.get("name").and_then(Value::as_str) else {
        return err(id, INVALID_PARAMS, "missing tool name");
    };
    let arguments = object.get("arguments").unwrap_or(&Value::Null);
    if !arguments.is_null() && !arguments.is_object() {
        return err(id, INVALID_PARAMS, "tool arguments must be an object");
    }

    match name {
        "ping" => {
            let message = arguments
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("pong");
            if message.len() > MAX_PING_MESSAGE_BYTES || message.chars().any(char::is_control) {
                return err(id, INVALID_PARAMS, "ping message is invalid or too long");
            }
            let structured = json!({"message": message});
            ok(
                id,
                json!({
                    "content": [{"type": "text", "text": message}],
                    "structuredContent": structured,
                    "isError": false
                }),
            )
        }
        unknown => err(
            id,
            INVALID_PARAMS,
            &format!("unknown tool: {unknown}"),
        ),
    }
}

fn protocol_header_supported(headers: &HeaderMap) -> bool {
    headers
        .get(PROTOCOL_VERSION_HEADER)
        .map(|value| {
            value
                .to_str()
                .ok()
                .is_some_and(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(&version))
        })
        .unwrap_or(true)
}

fn negotiated_protocol_version(params: &Value) -> String {
    params
        .get("protocolVersion")
        .and_then(Value::as_str)
        .filter(|version| SUPPORTED_PROTOCOL_VERSIONS.contains(version))
        .unwrap_or(PROTOCOL_VERSION)
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::{Body, to_bytes};

    fn parse_request(body: &str) -> JsonRpcRequest {
        serde_json::from_str(body).expect("valid test request")
    }

    async fn response_json(response: Response) -> Value {
        let bytes = to_bytes(response.into_body(), MAX_REQUEST_BYTES)
            .await
            .expect("response body");
        serde_json::from_slice(&bytes).expect("JSON-RPC response")
    }

    #[tokio::test]
    async fn every_idless_notification_is_accepted_without_a_body() {
        for body in [
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            r#"{"jsonrpc":"2.0","method":"notifications/cancelled","params":{"requestId":1}}"#,
            r#"{"jsonrpc":"2.0","method":"future/notification"}"#,
        ] {
            let response = handle(HeaderMap::new(), Json(parse_request(body))).await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let bytes = to_bytes(response.into_body(), MAX_REQUEST_BYTES)
                .await
                .expect("response body");
            assert!(bytes.is_empty());
        }
    }

    #[tokio::test]
    async fn rejects_null_fractional_boolean_object_and_array_ids() {
        for id in ["null", "1.5", "true", "{}", "[]"] {
            let body = format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#);
            let response = handle(HeaderMap::new(), Json(parse_request(&body))).await;
            assert_eq!(response.status(), StatusCode::BAD_REQUEST);
            let value = response_json(response).await;
            assert_eq!(value["error"]["code"], INVALID_REQUEST);
            assert!(value["id"].is_null());
        }
    }

    #[tokio::test]
    async fn validates_json_rpc_and_protocol_versions() {
        let invalid_json_rpc = handle(
            HeaderMap::new(),
            Json(parse_request(
                r#"{"jsonrpc":"1.0","id":"bad","method":"ping"}"#,
            )),
        )
        .await;
        assert_eq!(invalid_json_rpc.status(), StatusCode::BAD_REQUEST);
        assert_eq!(response_json(invalid_json_rpc).await["id"], "bad");

        let mut headers = HeaderMap::new();
        headers.insert(
            PROTOCOL_VERSION_HEADER,
            "1900-01-01".parse().expect("header value"),
        );
        let invalid_protocol = handle(
            headers,
            Json(parse_request(
                r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#,
            )),
        )
        .await;
        assert_eq!(invalid_protocol.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn initialize_negotiates_supported_versions_and_preserves_string_ids() {
        let response = handle(
            HeaderMap::new(),
            Json(parse_request(
                r#"{"jsonrpc":"2.0","id":"init","method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"contract","version":"1"}}}"#,
            )),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let value = response_json(response).await;
        assert_eq!(value["id"], "init");
        assert_eq!(value["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(value["result"]["capabilities"]["tools"]["listChanged"], false);
    }

    #[test]
    fn tool_catalog_is_unique_bounded_and_non_destructive() {
        let tools = tool_catalog();
        let tools = tools.as_array().expect("tool array");
        assert_eq!(tools.len(), 1);
        assert_eq!(tools[0]["name"], "ping");
        assert_eq!(tools[0]["inputSchema"]["type"], "object");
        assert_eq!(tools[0]["inputSchema"]["additionalProperties"], false);
        assert_eq!(tools[0]["annotations"]["readOnlyHint"], true);
        assert_eq!(tools[0]["annotations"]["destructiveHint"], false);
        assert_eq!(tools[0]["annotations"]["idempotentHint"], true);
        assert_eq!(tools[0]["annotations"]["openWorldHint"], false);
    }

    #[test]
    fn ping_tool_returns_structured_content_and_rejects_unbounded_messages() {
        let result = call_tool(
            json!(7),
            &json!({"name": "ping", "arguments": {"message": "hello"}}),
        );
        assert_eq!(result["result"]["structuredContent"]["message"], "hello");
        assert_eq!(result["result"]["isError"], false);

        let too_long = "x".repeat(MAX_PING_MESSAGE_BYTES + 1);
        let error = call_tool(
            json!(8),
            &json!({"name": "ping", "arguments": {"message": too_long}}),
        );
        assert_eq!(error["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn unknown_methods_and_tools_are_typed_errors() {
        assert_eq!(
            dispatch(json!(1), "unknown/method", &json!({}))["error"]["code"],
            METHOD_NOT_FOUND
        );
        assert_eq!(
            call_tool(json!(2), &json!({"name": "unknown", "arguments": {}}))["error"]["code"],
            INVALID_PARAMS
        );
    }
}
