//! Strict flags2env startup configuration for the HTTP MCP server.

use std::{
    error::Error,
    io,
    path::{Path, PathBuf},
};

use flags2env::BundledFlags2Env;
use tracing_subscriber::EnvFilter;

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_LOG_FILTER: &str = "info,act_mcp_server=debug";

#[derive(Debug)]
pub struct StartupFlags {
    pub port: u16,
    pub log_filter: EnvFilter,
}

fn invalid_input(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidInput, message.into())
}

pub fn parse_cli_flags(
    argv: &[String],
    config_path: &Path,
) -> Result<StartupFlags, Box<dyn Error>> {
    let config_path = config_path
        .to_str()
        .ok_or_else(|| invalid_input(".cli-flags.toml path is not valid UTF-8"))?;
    let parser = BundledFlags2Env::new();
    parser.audit_config(Some(config_path)).map_err(|error| {
        invalid_input(format!("flags-2-env configuration audit failed: {error}"))
    })?;
    let parsed = parser
        .parse_structured(argv, Some(config_path))
        .map_err(|error| invalid_input(format!("flags-2-env parse failed: {error}")))?;

    if !parsed.unknown_options.is_empty() {
        return Err(invalid_input(format!(
            "unknown command-line option(s): {}",
            parsed.unknown_options.join(", ")
        ))
        .into());
    }
    if !parsed.errors.is_empty() {
        return Err(invalid_input(format!(
            "invalid command-line value(s): {}",
            parsed.errors.join("; ")
        ))
        .into());
    }
    if !parsed.extras.is_empty() {
        return Err(invalid_input(format!(
            "unexpected positional argument(s): {}",
            parsed.extras.join(", ")
        ))
        .into());
    }

    let port = parsed
        .flags
        .get("PORT")
        .map(String::as_str)
        .unwrap_or("")
        .trim();
    let port = if port.is_empty() {
        DEFAULT_PORT
    } else {
        port.parse::<u16>()
            .map_err(|_| invalid_input("--port must be an integer between 1 and 65535"))?
    };
    if port == 0 {
        return Err(invalid_input("--port must be between 1 and 65535").into());
    }

    let filter = parsed
        .flags
        .get("RUST_LOG")
        .map(String::as_str)
        .unwrap_or(DEFAULT_LOG_FILTER);
    let log_filter = EnvFilter::try_new(filter)
        .map_err(|error| invalid_input(format!("invalid --log-filter value: {error}")))?;

    Ok(StartupFlags { port, log_filter })
}

pub fn resolve_config_path() -> Result<PathBuf, Box<dyn Error>> {
    if let Some(path) = std::env::var_os("ACT_MCP_FLAGS_CONFIG").filter(|value| !value.is_empty()) {
        let path = PathBuf::from(path);
        if path.is_file() {
            return Ok(path);
        }
        return Err(
            invalid_input("ACT_MCP_FLAGS_CONFIG does not point to a readable file").into(),
        );
    }

    let mut candidates = Vec::new();
    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join(".cli-flags.toml"));
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(parent) = executable.parent() {
            candidates.push(parent.join(".cli-flags.toml"));
            candidates.push(parent.join("../share/act-mcp-server/.cli-flags.toml"));
        }
    }

    candidates
        .into_iter()
        .find(|candidate| candidate.is_file())
        .ok_or_else(|| {
            invalid_input(
                "cannot locate .cli-flags.toml; set ACT_MCP_FLAGS_CONFIG to its path",
            )
            .into()
        })
}

pub fn process_startup_flags() -> Result<StartupFlags, Box<dyn Error>> {
    let argv = std::env::args().collect::<Vec<_>>();
    let config_path = resolve_config_path()?;
    parse_cli_flags(&argv, &config_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_path() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".cli-flags.toml")
    }

    #[test]
    fn accepts_declared_operational_flags() {
        let argv = vec![
            "act-mcp-server".to_owned(),
            "--port=9090".to_owned(),
            "--log-filter=debug,hyper=warn".to_owned(),
        ];
        let flags = parse_cli_flags(&argv, &config_path()).expect("valid operational flags");
        assert_eq!(flags.port, 9090);
        assert!(flags.log_filter.to_string().contains("debug"));
    }

    #[test]
    fn rejects_secret_bearing_flags() {
        let argv = vec![
            "act-mcp-server".to_owned(),
            "--server-auth-secret=must-remain-environment-only".to_owned(),
        ];
        let error = parse_cli_flags(&argv, &config_path())
            .expect_err("secret-bearing option must remain unknown")
            .to_string();
        assert!(error.contains("unknown command-line option"));
    }

    #[test]
    fn rejects_invalid_values() {
        let zero_port = vec!["act-mcp-server".to_owned(), "--port=0".to_owned()];
        assert!(parse_cli_flags(&zero_port, &config_path()).is_err());

        let bad_filter = vec![
            "act-mcp-server".to_owned(),
            "--log-filter=[invalid".to_owned(),
        ];
        assert!(parse_cli_flags(&bad_filter, &config_path()).is_err());
    }
}
