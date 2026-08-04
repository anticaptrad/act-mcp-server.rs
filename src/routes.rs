//! HTTP surface: k8s probes plus the bounded MCP JSON-RPC endpoint.

use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Request},
    http::{HeaderValue, header},
    middleware::{self, Next},
    response::Response,
    routing::{get, post},
};
use serde_json::{Value, json};

use crate::auth;
use crate::mcp;
use crate::state::AppState;

pub fn router(state: AppState) -> Router {
    // The MCP surface is tool execution, so it sits behind the shared secret,
    // exact Host, and browser Origin gate. Probes stay public because the
    // kubelet sends no auth headers.
    let protected = Router::new()
        .route("/mcp", post(mcp::handle))
        .layer(DefaultBodyLimit::max(mcp::MAX_REQUEST_BYTES))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_server_auth,
        ));

    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
        .merge(protected)
        .with_state(state)
        .layer(middleware::from_fn(add_security_headers))
}

async fn add_security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response.headers_mut().insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    response.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    response.headers_mut().insert(
        "content-security-policy",
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );
    response
}

async fn health() -> Json<Value> {
    Json(json!({"status": "ok"}))
}

async fn ready() -> Json<Value> {
    Json(json!({"ready": true}))
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode, header},
    };
    use tower::ServiceExt;

    const SECRET: &str = "012345678901234567890123";
    const HOST: &str = "act-mcp-server:8080";

    fn test_state() -> AppState {
        AppState {
            auth_secret: Some(SECRET.to_owned()),
            allowed_hosts: BTreeSet::from([HOST.to_owned()]),
            allowed_origins: BTreeSet::from(["https://console.example".to_owned()]),
        }
    }

    fn mcp_request(body: &str) -> Request<Body> {
        Request::post("/mcp")
            .header(header::HOST, HOST)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-server-auth", SECRET)
            .body(Body::from(body.to_owned()))
            .expect("request")
    }

    fn assert_security_headers(response: &Response) {
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(
            response.headers()[header::X_CONTENT_TYPE_OPTIONS],
            "nosniff"
        );
        assert_eq!(response.headers()[header::REFERRER_POLICY], "no-referrer");
        assert_eq!(
            response.headers()["content-security-policy"],
            "default-src 'none'; frame-ancestors 'none'"
        );
    }

    #[tokio::test]
    async fn probes_are_public_but_mcp_requires_host_and_shared_secret() {
        let app = router(test_state());
        let health = app
            .clone()
            .oneshot(
                Request::get("/health")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(health.status(), StatusCode::OK);
        assert_security_headers(&health);

        let missing_host = app
            .clone()
            .oneshot(
                Request::post("/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-server-auth", SECRET)
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(missing_host.status(), StatusCode::MISDIRECTED_REQUEST);
        assert_security_headers(&missing_host);

        let unauthorized = app
            .oneshot(
                Request::post("/mcp")
                    .header(header::HOST, HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(unauthorized.status(), StatusCode::UNAUTHORIZED);
        assert_security_headers(&unauthorized);
    }

    #[tokio::test]
    async fn host_lookalikes_and_browser_origins_fail_closed() {
        let app = router(test_state());
        let wrong_host = app
            .clone()
            .oneshot(
                Request::post("/mcp")
                    .header(header::HOST, "act-mcp-server.attacker:8080")
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-server-auth", SECRET)
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(wrong_host.status(), StatusCode::MISDIRECTED_REQUEST);

        let rejected = app
            .clone()
            .oneshot(
                Request::post("/mcp")
                    .header(header::HOST, HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-server-auth", SECRET)
                    .header(header::ORIGIN, "https://evil.example")
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(rejected.status(), StatusCode::FORBIDDEN);

        let accepted = app
            .oneshot(
                Request::post("/mcp")
                    .header(header::HOST, HOST)
                    .header(header::CONTENT_TYPE, "application/json")
                    .header("x-server-auth", SECRET)
                    .header(header::ORIGIN, "https://console.example/")
                    .body(Body::from(r#"{"jsonrpc":"2.0","id":1,"method":"ping"}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(accepted.status(), StatusCode::OK);
        assert_security_headers(&accepted);
    }

    #[tokio::test]
    async fn tools_list_and_notification_behavior_work_through_the_real_router() {
        let app = router(test_state());
        let response = app
            .clone()
            .oneshot(mcp_request(
                r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        assert_security_headers(&response);
        let bytes = to_bytes(response.into_body(), mcp::MAX_REQUEST_BYTES)
            .await
            .expect("body");
        let value: Value = serde_json::from_slice(&bytes).expect("JSON");
        assert_eq!(value["result"]["tools"][0]["name"], "ping");

        let notification = app
            .oneshot(mcp_request(
                r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            ))
            .await
            .expect("response");
        assert_eq!(notification.status(), StatusCode::ACCEPTED);
        assert_security_headers(&notification);
        let bytes = to_bytes(notification.into_body(), mcp::MAX_REQUEST_BYTES)
            .await
            .expect("body");
        assert!(bytes.is_empty());
    }

    #[tokio::test]
    async fn request_body_is_bounded() {
        let oversized = format!(
            r#"{{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{{"name":"ping","arguments":{{"message":"{}"}}}}}}"#,
            "x".repeat(mcp::MAX_REQUEST_BYTES)
        );
        let response = router(test_state())
            .oneshot(mcp_request(&oversized))
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_security_headers(&response);
    }
}
