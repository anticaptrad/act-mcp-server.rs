//! Shared application state passed to handlers and middleware.

use std::collections::BTreeSet;

#[derive(Clone)]
pub struct AppState {
    /// Shared secret guarding the MCP surface. `None` fails protected
    /// requests closed rather than serving them.
    pub auth_secret: Option<String>,
    /// Normalized browser origins explicitly allowed to call `/mcp`.
    pub allowed_origins: BTreeSet<String>,
}
