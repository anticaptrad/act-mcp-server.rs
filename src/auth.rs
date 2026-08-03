//! Shared-secret, Host, and browser-Origin gate for the MCP surface.
//!
//! `tools/call` is remote tool execution. Today the catalog is intentionally
//! tiny, but an unauthenticated MCP endpoint is a standing invitation to every
//! future tool. Requests therefore fail closed when the shared secret is absent,
//! compare secrets in constant time, require an exact Host authority, and reject
//! browser origins unless they are explicitly allowlisted.
//!
//! Probes stay public — the kubelet sends no auth headers.

use std::collections::BTreeSet;

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::config::{normalize_host, normalize_origin};
use crate::state::AppState;

/// Header carrying the shared secret, matching the platform convention.
const HEADER: &str = "x-server-auth";
const MAX_PRESENTED_SECRET_BYTES: usize = 4 * 1024;

/// Compare without leaking content through timing.
///
/// Both sides are hashed to a fixed width first so the comparison is over equal
/// lengths and the secret's length is not itself an oracle.
fn secrets_match(presented: &str, expected: &str) -> bool {
    let a = Sha256::digest(presented.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    a.ct_eq(&b).into()
}

fn host_allowed(headers: &HeaderMap, allowed_hosts: &BTreeSet<String>) -> bool {
    headers
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .and_then(normalize_host)
        .is_some_and(|host| allowed_hosts.contains(&host))
}

fn origin_allowed(headers: &HeaderMap, allowed_origins: &BTreeSet<String>) -> bool {
    let Some(raw) = headers.get(header::ORIGIN) else {
        // Native MCP clients normally send no Origin. The check is specifically
        // for browser traffic; Host validation remains mandatory for all HTTP.
        return true;
    };
    let Ok(raw) = raw.to_str() else {
        return false;
    };
    normalize_origin(raw).is_some_and(|origin| allowed_origins.contains(&origin))
}

fn presented_secret(headers: &HeaderMap) -> Option<&str> {
    let value = headers.get(HEADER)?.to_str().ok()?;
    if value.is_empty()
        || value.len() > MAX_PRESENTED_SECRET_BYTES
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        return None;
    }
    Some(value)
}

/// Reject any MCP request without the configured secret, an exact Host, or a
/// trusted browser Origin.
///
/// Fails closed: with no secret configured the endpoint answers 503 rather than
/// serving. Treating "unconfigured" as "open" is how a tool-execution surface
/// ends up exposed by a missing environment variable.
pub async fn require_server_auth(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let expected = state.auth_secret.as_ref().ok_or_else(|| {
        tracing::error!("SERVER_AUTH_SECRET not configured; refusing MCP request");
        StatusCode::SERVICE_UNAVAILABLE
    })?;

    if !host_allowed(request.headers(), &state.allowed_hosts) {
        return Err(StatusCode::MISDIRECTED_REQUEST);
    }
    let presented = presented_secret(request.headers()).ok_or(StatusCode::UNAUTHORIZED)?;
    if !secrets_match(presented, expected) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if !origin_allowed(request.headers(), &state.allowed_origins) {
        return Err(StatusCode::FORBIDDEN);
    }

    Ok(next.run(request).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    #[test]
    fn secret_comparison_accepts_only_exact_values() {
        assert!(secrets_match(
            "correct-horse-battery-staple",
            "correct-horse-battery-staple"
        ));
        assert!(!secrets_match(
            "correct-horse-battery-staple",
            "correct-horse-battery-staplf"
        ));
        assert!(!secrets_match("short", "correct-horse-battery-staple"));
    }

    #[test]
    fn presented_secret_rejects_whitespace_and_oversized_values() {
        let mut headers = HeaderMap::new();
        headers.insert(HEADER, HeaderValue::from_static("machine-secret-value"));
        assert_eq!(presented_secret(&headers), Some("machine-secret-value"));

        headers.insert(HEADER, HeaderValue::from_static("two words"));
        assert_eq!(presented_secret(&headers), None);

        let oversized = HeaderValue::from_str(&"a".repeat(MAX_PRESENTED_SECRET_BYTES + 1))
            .expect("valid header bytes");
        headers.insert(HEADER, oversized);
        assert_eq!(presented_secret(&headers), None);
    }

    #[test]
    fn host_gate_requires_an_exact_normalized_authority() {
        let allowed = BTreeSet::from(["act-mcp-server:8080".to_owned()]);
        let mut headers = HeaderMap::new();
        assert!(!host_allowed(&headers, &allowed));
        headers.insert(
            header::HOST,
            HeaderValue::from_static("ACT-MCP-SERVER:8080"),
        );
        assert!(host_allowed(&headers, &allowed));
        headers.insert(
            header::HOST,
            HeaderValue::from_static("act-mcp-server.attacker:8080"),
        );
        assert!(!host_allowed(&headers, &allowed));
    }

    #[test]
    fn origin_gate_allows_native_clients_and_exact_normalized_origins() {
        let allowed = BTreeSet::from(["https://console.example".to_owned()]);
        assert!(origin_allowed(&HeaderMap::new(), &allowed));

        let mut headers = HeaderMap::new();
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://console.example/"),
        );
        assert!(origin_allowed(&headers, &allowed));

        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!origin_allowed(&headers, &allowed));
    }
}
