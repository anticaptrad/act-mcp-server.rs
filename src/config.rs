//! Runtime configuration derived from an immutable environment snapshot.

use std::collections::BTreeSet;

use anyhow::{Context, bail};
use axum::http::{Uri, uri::Authority};

use crate::env_map::{value, EnvMap};

const MIN_AUTH_SECRET_BYTES: usize = 24;
const MAX_AUTH_SECRET_BYTES: usize = 4 * 1024;
const MAX_ALLOWED_ENTRIES: usize = 64;
const DEFAULT_ALLOWED_HOSTS: &str = "localhost,localhost:8080,127.0.0.1,127.0.0.1:8080,[::1],[::1]:8080,act-mcp-server,act-mcp-server:8080";

#[derive(Debug, Clone)]
pub struct Config {
    pub port: u16,
    pub service_name: String,
    pub auth_secret: Option<String>,
    pub allowed_hosts: BTreeSet<String>,
    pub allowed_origins: BTreeSet<String>,
}

impl Config {
    pub fn from_env_map(env: &EnvMap, port_override: Option<u16>) -> anyhow::Result<Self> {
        let port = match port_override {
            Some(port) => port,
            None => value(env, "PORT")
                .map(|raw| raw.parse().context("PORT must be a valid u16"))
                .transpose()?
                .unwrap_or(8080),
        };

        let service_name = value(env, "OTEL_SERVICE_NAME")
            .unwrap_or("act-mcp-server")
            .to_owned();

        let auth_secret = value(env, "SERVER_AUTH_SECRET")
            .map(str::to_owned)
            .map(validate_auth_secret)
            .transpose()?;

        let allowed_hosts = parse_allowed_hosts(
            value(env, "MCP_ALLOWED_HOSTS").unwrap_or(DEFAULT_ALLOWED_HOSTS),
        )?;
        let allowed_origins = parse_allowed_origins(
            value(env, "MCP_ALLOWED_ORIGINS").unwrap_or_default(),
        )?;

        Ok(Self {
            port,
            service_name,
            auth_secret,
            allowed_hosts,
            allowed_origins,
        })
    }
}

fn validate_auth_secret(value: String) -> anyhow::Result<String> {
    if !(MIN_AUTH_SECRET_BYTES..=MAX_AUTH_SECRET_BYTES).contains(&value.len())
        || value
            .chars()
            .any(|character| character.is_control() || character.is_whitespace())
    {
        bail!(
            "SERVER_AUTH_SECRET must be {MIN_AUTH_SECRET_BYTES}..={MAX_AUTH_SECRET_BYTES} bytes and contain no whitespace or control characters"
        );
    }
    Ok(value)
}

pub(crate) fn normalize_host(raw: &str) -> Option<String> {
    let authority: Authority = raw.parse().ok()?;
    let normalized = authority.as_str().to_ascii_lowercase();
    if normalized.len() > 512
        || normalized.contains('@')
        || normalized.chars().any(char::is_whitespace)
    {
        return None;
    }
    Some(normalized)
}

fn parse_allowed_hosts(raw: &str) -> anyhow::Result<BTreeSet<String>> {
    let mut hosts = BTreeSet::new();
    for candidate in raw.split(',').map(str::trim).filter(|value| !value.is_empty()) {
        if candidate == "*" {
            bail!("MCP_ALLOWED_HOSTS must not contain '*'");
        }
        let host = normalize_host(candidate)
            .ok_or_else(|| anyhow::anyhow!("MCP_ALLOWED_HOSTS contains an invalid authority"))?;
        hosts.insert(host);
    }
    if hosts.is_empty() || hosts.len() > MAX_ALLOWED_ENTRIES {
        bail!("MCP_ALLOWED_HOSTS must contain 1..={MAX_ALLOWED_ENTRIES} exact authorities");
    }
    Ok(hosts)
}

pub(crate) fn normalize_origin(raw: &str) -> Option<String> {
    let uri: Uri = raw.parse().ok()?;
    let scheme = uri.scheme_str()?.to_ascii_lowercase();
    if !matches!(scheme.as_str(), "http" | "https") {
        return None;
    }
    let authority = normalize_host(uri.authority()?.as_str())?;
    if uri
        .path_and_query()
        .is_some_and(|path_and_query| path_and_query.as_str() != "/")
    {
        return None;
    }
    Some(format!("{scheme}://{authority}"))
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
    if origins.len() > MAX_ALLOWED_ENTRIES {
        bail!("MCP_ALLOWED_ORIGINS may contain at most {MAX_ALLOWED_ENTRIES} origins");
    }
    Ok(origins)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_env() -> EnvMap {
        EnvMap::from([("MCP_ALLOWED_HOSTS".into(), "localhost:8080".into())])
    }

    #[test]
    fn explicit_port_override_wins_without_process_reads() {
        let config = Config::from_env_map(&base_env(), Some(9191)).expect("valid configuration");
        assert_eq!(config.port, 9191);
    }

    #[test]
    fn auth_secret_shape_is_strict_and_bounded() {
        assert!(validate_auth_secret("a".repeat(MIN_AUTH_SECRET_BYTES)).is_ok());
        for bad in [
            "short".to_owned(),
            "a secret with whitespace and enough length".to_owned(),
            "a".repeat(MAX_AUTH_SECRET_BYTES + 1),
        ] {
            assert!(validate_auth_secret(bad).is_err());
        }
    }

    #[test]
    fn hosts_are_exact_normalized_and_wildcards_fail_closed() {
        assert_eq!(
            parse_allowed_hosts("Console.Example:443, localhost:8080").expect("valid hosts"),
            BTreeSet::from([
                "console.example:443".to_owned(),
                "localhost:8080".to_owned(),
            ])
        );
        for bad in ["*", "https://console.example", "user@console.example", "a b"] {
            assert!(parse_allowed_hosts(bad).is_err(), "should reject {bad:?}");
        }
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
