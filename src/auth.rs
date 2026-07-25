//! Shared-secret gate for the MCP surface.
//!
//! `tools/call` is remote tool execution. Today the catalog is a health probe,
//! but an unauthenticated MCP endpoint is a standing invitation to whatever can
//! reach the Service, and the tools it exposes only ever grow. The cluster's own
//! MCP servers all sit behind a secret; this matches that posture.
//!
//! Probes stay public — the kubelet sends no headers.

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use crate::state::AppState;

/// Header carrying the shared secret, matching the platform convention.
const HEADER: &str = "x-server-auth";

/// Compare without leaking content through timing.
///
/// Both sides are hashed to a fixed width first so the comparison is over equal
/// lengths and the secret's length is not itself an oracle.
fn secrets_match(presented: &str, expected: &str) -> bool {
    let a = Sha256::digest(presented.as_bytes());
    let b = Sha256::digest(expected.as_bytes());
    a.ct_eq(&b).into()
}

/// Reject any MCP request without the configured secret.
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

    let presented = request
        .headers()
        .get(HEADER)
        .and_then(|value| value.to_str().ok())
        .ok_or(StatusCode::UNAUTHORIZED)?;

    if !secrets_match(presented, expected) {
        return Err(StatusCode::UNAUTHORIZED);
    }

    Ok(next.run(request).await)
}
