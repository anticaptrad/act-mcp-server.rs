//! Runtime configuration sourced from the environment (no `.env` loader).

use std::collections::BTreeSet;

use anyhow::{Context, bail};
use axum::http::Uri;

const MIN_AUTH_SECRET_BYTES: usize = 24;

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub service_name: String,
    /// Shared secret guarding the MCP surface. `None` keeps `/mcp` fail-closed.
    pub auth_secret: Option<String>,
    /// Browser origins explicitly permitted to call the Streamable HTTP endpoint.
    pub allowed_origins: BTreeSet<String>,
}

impl Config {
    pub fn from_env_with_port(port_override: Option<u16>) -> anyhow::Result<Self> {
        let port = match port_override {
            Some(port) => port,
            None => std::env::var("PORT")
                .ok()
                .map(|value| value.parse().context("PORT must be a valid u16"))
                .transpose()?
                .unwrap_or(8080),
        };

        let service_name =
            std::env::var("OTEL_SERVICE_NAME").unwrap_or_else(|_| "act-mcp-server".to_string());

        let auth_secret = std::env::var("SERVER_AUTH_SECRET")
            .ok()
            .filter(|value| !value.is_empty());
        if auth_secret
            .as_ref()
            .is_some_and(|secret| secret.len() < MIN_AUTH_SECRET_BYTES)
        {
            bail!("SERVER_AUTH_SECRET must be at least {MIN_AUTH_SECRET_BYTES} bytes");
        }

        let allowed_origins = parse_allowed_origins(
            &std::env::var("MCP_ALLOWED_ORIGINS").unwrap_or_default(),
        )?;

        Ok(Self {
            port,
            service_name,
            auth_secret,
            allowed_origins,
        })
    }
}

pub(crate) fn normalize_origin(raw: &str) -> Option<String> {
    let uri: Uri = raw.parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = uri.authority()?.as_str();
    if authority.contains('@') {
        return None;
    }
    if uri
        .path_and_query()
        .is_some_and(|path_and_query| path_and_query.as_str() != "/")
    {
        return None;
    }
    Some(format!("{scheme}://{}", authority.to_ascii_lowercase()))
}

fn parse_allowed_origins(raw: &str) -> anyhow::Result<BTreeSet<String>> {
    let mut origins = BTreeSet::new();
    for candidate in raw.split(',').map(str::trim).filter(|value| !value.is_empty()) {
        if candidate == "*" {
            bail!("MCP_ALLOWED_ORIGINS must not contain '*'");
        }
        let origin = normalize_origin(candidate)
            .ok_or_else(|| anyhow::anyhow!("MCP_ALLOWED_ORIGINS contains an invalid origin"))?;
        origins.insert(origin);
    }
    Ok(origins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_port_override_wins_without_exposing_secrets() {
        let config = Config::from_env_with_port(Some(9191)).expect("valid configuration");
        assert_eq!(config.port, 9191);
    }

    #[test]
    fn origins_are_normalized_and_wildcards_fail_closed() {
        assert_eq!(
            parse_allowed_origins("https://Console.Example/, http://localhost:3000")
                .expect("valid origins"),
            BTreeSet::from([
                "http://localhost:3000".to_owned(),
                "https://console.example".to_owned(),
            ])
        );
        assert!(parse_allowed_origins("*").is_err());
        assert!(parse_allowed_origins("https://console.example/path").is_err());
        assert!(parse_allowed_origins("file:///tmp/socket").is_err());
    }
}
