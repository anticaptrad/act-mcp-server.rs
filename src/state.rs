//! Shared application state passed to handlers and middleware.

#[derive(Clone)]
pub struct AppState {
    /// Shared secret guarding the MCP surface. `None` fails protected
    /// requests closed rather than serving them.
    pub auth_secret: Option<String>,
}
