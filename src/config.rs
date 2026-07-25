//! Runtime configuration sourced from the environment (no `.env` — `dotenv` is
//! blacklisted, see `agents.md`).

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub service_name: String,
    /// Shared secret guarding the MCP surface.
    pub auth_secret: Option<String>,
}

impl Config {
    pub fn from_env() -> Self {
        let port = std::env::var("PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(8080);

        let service_name =
            std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "act-mcp-server".to_string());

        let auth_secret = std::env::var("SERVER_AUTH_SECRET")
            .ok()
            .filter(|s| !s.is_empty());

        Self {
            port,
            service_name,
            auth_secret,
        }
    }
}
