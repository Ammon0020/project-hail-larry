//! Claude Desktop-compatible MCP configuration and on-demand health checks.
//!
//! Config lives in `<state_dir>/mcp.json`. Typed saves produce readable JSON;
//! callers that edit raw JSON can validate and preserve their exact bytes with
//! [`File::save_raw`].

use std::collections::BTreeMap;
use std::fs;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_client_protocol::schema::v1::{
    EnvVariable, HttpHeader, McpCapabilities, McpServer, McpServerHttp, McpServerSse,
    McpServerStdio,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;
use crate::fsutil;

/// Current on-disk MCP configuration format version.
pub const CURRENT_VERSION: u32 = 1;
const CONFIG_FILE_NAME: &str = "mcp.json";
const CONFIG_FILE_PERM: u32 = 0o600;
const DEFAULT_HEALTH_TIMEOUT: Duration = Duration::from_secs(2);

/// A configured MCP server, compatible with Claude Desktop's server object.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerConfig {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub transport: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub env: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub url: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, String>,
    /// `None` is enabled, preserving Claude-compatible default-on behavior.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enabled: Option<bool>,
}

/// MCP configuration envelope stored in `mcp.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct File {
    #[serde(rename = "$schema", default, skip_serializing_if = "String::is_empty")]
    pub schema: String,
    #[serde(default)]
    pub version: u32,
    #[serde(rename = "mcpServers", default)]
    pub mcp_servers: BTreeMap<String, ServerConfig>,
}

impl Default for File {
    fn default() -> Self {
        Self::new()
    }
}

impl File {
    /// Returns an empty valid configuration envelope.
    #[must_use]
    pub fn new() -> Self {
        Self {
            schema: String::new(),
            version: CURRENT_VERSION,
            mcp_servers: BTreeMap::new(),
        }
    }

    /// Returns the state-dir MCP config path.
    pub fn path() -> Result<PathBuf, McpError> {
        Ok(Config::resolved_state_dir()?.join(CONFIG_FILE_NAME))
    }

    /// Loads a config; a missing file is an empty valid envelope.
    pub fn load(path: &Path) -> Result<Self, McpError> {
        match fs::read(path) {
            Ok(raw) => Self::parse(&raw),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(Self::new()),
            Err(error) => Err(error.into()),
        }
    }

    /// Parses a raw JSON document, normalizing an omitted version.
    pub fn parse(raw: &[u8]) -> Result<Self, McpError> {
        let mut file: Self = serde_json::from_slice(raw)?;
        if file.version == 0 {
            file.version = CURRENT_VERSION;
        }
        Ok(file)
    }

    /// Loads exact raw bytes, returning a valid empty JSON envelope if missing.
    pub fn load_raw(path: &Path) -> Result<Vec<u8>, McpError> {
        match fs::read(path) {
            Ok(raw) => Ok(raw),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                Ok(b"{\n  \"version\": 1,\n  \"mcpServers\": {}\n}\n".to_vec())
            }
            Err(error) => Err(error.into()),
        }
    }

    /// Saves this typed config atomically with owner-only permissions.
    pub fn save(&mut self, path: &Path) -> Result<(), McpError> {
        if self.version == 0 {
            self.version = CURRENT_VERSION;
        }
        fsutil::atomic_write(
            path,
            &serde_json::to_vec_pretty(self)?,
            Some(CONFIG_FILE_PERM),
        )?;
        Ok(())
    }

    /// Validates and atomically saves raw JSON without changing its formatting.
    pub fn save_raw(path: &Path, raw: &[u8]) -> Result<(), McpError> {
        let file = Self::parse(raw)?;
        if file.version != CURRENT_VERSION {
            return Err(McpError::UnsupportedVersion(file.version));
        }
        fsutil::atomic_write(path, raw, Some(CONFIG_FILE_PERM))?;
        Ok(())
    }

    /// Inserts or replaces a server configuration.
    pub fn upsert(&mut self, name: impl Into<String>, config: ServerConfig) {
        self.mcp_servers.insert(name.into(), config);
    }

    /// Removes a server, returning an error when it is absent.
    pub fn remove(&mut self, name: &str) -> Result<ServerConfig, McpError> {
        self.mcp_servers
            .remove(name)
            .ok_or_else(|| McpError::ServerNotFound(name.to_owned()))
    }

    /// Returns enabled servers, in deterministic name order.
    #[must_use]
    pub fn enabled(&self) -> BTreeMap<&str, &ServerConfig> {
        self.mcp_servers
            .iter()
            .filter(|(_, config)| config.enabled != Some(false))
            .map(|(name, config)| (name.as_str(), config))
            .collect()
    }

    /// Converts enabled, capability-supported servers to exact ACP SDK types.
    pub fn to_acp(&self, capabilities: &McpCapabilities) -> Result<Vec<McpServer>, McpError> {
        let mut servers = Vec::new();
        for (name, config) in self.enabled() {
            let supported = match config.effective_transport()? {
                Transport::Http => capabilities.http,
                Transport::Sse => capabilities.sse,
                Transport::Stdio => true,
            };
            if supported {
                servers.push(config.to_acp(name)?);
            }
        }
        Ok(servers)
    }
}

/// Supported MCP transports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    Stdio,
    Http,
    Sse,
}

impl ServerConfig {
    /// Determines transport, inferring HTTP from a URL-only server.
    pub fn effective_transport(&self) -> Result<Transport, McpError> {
        match self.transport.to_ascii_lowercase().as_str() {
            "" if !self.url.is_empty() && self.command.is_empty() => Ok(Transport::Http),
            "" | "stdio" => Ok(Transport::Stdio),
            "http" => Ok(Transport::Http),
            "sse" => Ok(Transport::Sse),
            other => Err(McpError::UnsupportedTransport(other.to_owned())),
        }
    }

    /// Converts this server to its SDK schema representation.
    pub fn to_acp(&self, name: &str) -> Result<McpServer, McpError> {
        match self.effective_transport()? {
            Transport::Stdio => Ok(McpServer::Stdio(
                McpServerStdio::new(name, expand_env(&self.command))
                    .args(self.args.iter().map(|arg| expand_env(arg)).collect())
                    .env(map_to_env(&self.env)),
            )),
            Transport::Http => Ok(McpServer::Http(
                McpServerHttp::new(name, expand_env(&self.url))
                    .headers(map_to_headers(&self.headers)),
            )),
            Transport::Sse => Ok(McpServer::Sse(
                McpServerSse::new(name, expand_env(&self.url))
                    .headers(map_to_headers(&self.headers)),
            )),
        }
    }
}

fn map_to_env(values: &BTreeMap<String, String>) -> Vec<EnvVariable> {
    values
        .iter()
        .map(|(name, value)| EnvVariable::new(name, expand_env(value)))
        .collect()
}

fn map_to_headers(values: &BTreeMap<String, String>) -> Vec<HttpHeader> {
    values
        .iter()
        .map(|(name, value)| HttpHeader::new(name, expand_env(value)))
        .collect()
}

/// Expands `${NAME}` from the process environment; unset variables become empty.
#[must_use]
pub fn expand_env(value: &str) -> String {
    expand_env_with(value, |name| std::env::var(name).ok())
}

fn expand_env_with(value: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut result = String::with_capacity(value.len());
    let mut remaining = value;
    let mut expanded = false;
    while let Some(start) = remaining.find("${") {
        result.push_str(&remaining[..start]);
        let after_start = &remaining[start + 2..];
        let Some(end) = after_start.find('}') else {
            result.push_str(&remaining[start..]);
            break;
        };
        let name = &after_start[..end];
        if is_env_name(name) {
            result.push_str(&lookup(name).unwrap_or_default());
            expanded = true;
        } else {
            result.push_str(&remaining[start..start + 3 + end]);
        }
        remaining = &after_start[end + 1..];
    }
    if expanded || !result.is_empty() || remaining.is_empty() {
        result.push_str(remaining);
        result
    } else {
        value.to_owned()
    }
}

fn is_env_name(name: &str) -> bool {
    let mut chars = name.bytes();
    matches!(chars.next(), Some(b'A'..=b'Z' | b'a'..=b'z' | b'_'))
        && chars.all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

/// Health result for one configured server.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    pub name: String,
    pub enabled: bool,
    pub status: HealthStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

/// Health state used by the MCP settings UI.
#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Healthy,
    Unhealthy,
    Disabled,
    Unknown,
}

/// Checks every server in deterministic order. Zero uses a two-second timeout.
#[must_use]
pub fn check_health(file: &File, timeout: Duration) -> Vec<ServerStatus> {
    let timeout = if timeout.is_zero() {
        DEFAULT_HEALTH_TIMEOUT
    } else {
        timeout
    };
    file.mcp_servers
        .iter()
        .map(|(name, config)| check_server(name, config, timeout))
        .collect()
}

fn check_server(name: &str, config: &ServerConfig, timeout: Duration) -> ServerStatus {
    let enabled = config.enabled != Some(false);
    if !enabled {
        return ServerStatus {
            name: name.to_owned(),
            enabled,
            status: HealthStatus::Disabled,
            error: None,
        };
    }
    let result = match config.effective_transport() {
        Ok(Transport::Stdio) => check_stdio(config),
        Ok(Transport::Http | Transport::Sse) => check_network(config, timeout),
        Err(error) => Err(error.to_string()),
    };
    match result {
        Ok(()) => ServerStatus {
            name: name.to_owned(),
            enabled,
            status: HealthStatus::Healthy,
            error: None,
        },
        Err(error) => ServerStatus {
            name: name.to_owned(),
            enabled,
            status: if config.effective_transport().is_err() {
                HealthStatus::Unknown
            } else {
                HealthStatus::Unhealthy
            },
            error: Some(error),
        },
    }
}

fn check_stdio(config: &ServerConfig) -> Result<(), String> {
    let command = expand_env(&config.command);
    if command.is_empty() {
        return Err("no command configured".to_owned());
    }
    std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
        .map(|dir| dir.join(&command))
        .find(|candidate| is_executable(candidate))
        .map(|_| ())
        .ok_or_else(|| format!("executable not found: {command}"))
}

#[cfg(unix)]
fn is_executable(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;

    path.is_file()
        && fs::metadata(path)
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable(path: &Path) -> bool {
    path.is_file()
}

fn check_network(config: &ServerConfig, timeout: Duration) -> Result<(), String> {
    let raw_url = expand_env(&config.url);
    let parsed = reqwest::Url::parse(&raw_url).map_err(|error| format!("invalid URL: {error}"))?;
    let host = parsed
        .host_str()
        .ok_or_else(|| "invalid URL: missing host".to_owned())?;
    let port = parsed.port_or_known_default().unwrap_or(80);
    let address = (host, port)
        .to_socket_addrs()
        .map_err(|error| format!("connection failed: {error}"))?
        .next()
        .ok_or_else(|| "connection failed: no resolved address".to_owned())?;
    TcpStream::connect_timeout(&address, timeout)
        .map(|_| ())
        .map_err(|error| format!("connection failed: {error}"))
}

/// Errors from MCP config parsing, persistence, conversion, and mutation.
#[derive(Debug, Error)]
pub enum McpError {
    #[error("MCP config I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid MCP config JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unsupported MCP config version: {0}")]
    UnsupportedVersion(u32),
    #[error("unsupported MCP transport type: {0}")]
    UnsupportedTransport(String),
    #[error("MCP server not found: {0}")]
    ServerNotFound(String),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(command: &str) -> ServerConfig {
        ServerConfig {
            command: command.to_owned(),
            ..ServerConfig::default()
        }
    }

    #[test]
    fn load_missing_and_save_round_trip() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("nested/mcp.json");
        assert_eq!(File::load(&path).expect("missing load"), File::new());

        let mut file = File::new();
        file.upsert("github", config("echo"));
        file.save(&path).expect("save");
        assert_eq!(File::load(&path).expect("load"), file);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                fs::metadata(path).expect("metadata").permissions().mode() & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn raw_save_preserves_format_and_validates_version() {
        let temp = tempfile::tempdir().expect("temp dir");
        let path = temp.path().join("mcp.json");
        let raw = b"{ \"mcpServers\": { }, \"version\": 1 }\n";
        File::save_raw(&path, raw).expect("raw save");
        assert_eq!(File::load_raw(&path).expect("raw load"), raw);
        assert!(File::save_raw(&path, br#"{"version":2}"#).is_err());
    }

    #[test]
    fn enabled_transport_and_env_expansion_follow_go_behavior() {
        let mut file = File::new();
        file.upsert(
            "off",
            ServerConfig {
                enabled: Some(false),
                ..config("echo")
            },
        );
        file.upsert(
            "remote",
            ServerConfig {
                url: "https://${HOST}/mcp".to_owned(),
                headers: BTreeMap::from([(
                    "Authorization".to_owned(),
                    "Bearer ${TOKEN}".to_owned(),
                )]),
                ..ServerConfig::default()
            },
        );
        assert_eq!(
            file.enabled().keys().copied().collect::<Vec<_>>(),
            vec!["remote"]
        );
        assert!(matches!(
            file.mcp_servers["remote"].effective_transport(),
            Ok(Transport::Http)
        ));
        assert_eq!(
            expand_env_with("${SET}/${MISSING}_suffix", |name| (name == "SET")
                .then(|| "yes".to_owned())),
            "yes/_suffix"
        );
        assert_eq!(expand_env_with("${MISSING}/suffix", |_| None), "/suffix");
    }

    #[test]
    fn to_acp_filters_capabilities_and_preserves_sorted_maps() {
        let mut file = File::new();
        file.upsert("stdio", config("echo"));
        file.upsert(
            "http",
            ServerConfig {
                transport: "http".to_owned(),
                url: "https://example.test".to_owned(),
                ..ServerConfig::default()
            },
        );
        let servers = file
            .to_acp(&McpCapabilities::new())
            .expect("ACP conversion");
        assert!(matches!(servers.as_slice(), [McpServer::Stdio(_)]));
        let servers = file
            .to_acp(&McpCapabilities::new().http(true))
            .expect("ACP conversion");
        assert!(matches!(
            servers.as_slice(),
            [McpServer::Http(_), McpServer::Stdio(_)]
        ));
    }

    #[test]
    fn health_reports_healthy_disabled_unhealthy_and_unknown() {
        let mut file = File::new();
        file.upsert("a-healthy", config("sh"));
        file.upsert(
            "b-disabled",
            ServerConfig {
                enabled: Some(false),
                ..config("")
            },
        );
        file.upsert("c-unhealthy", config("missing-mcp-test-binary"));
        file.upsert(
            "d-unknown",
            ServerConfig {
                transport: "ftp".to_owned(),
                ..ServerConfig::default()
            },
        );
        let statuses = check_health(&file, Duration::from_millis(100));
        assert_eq!(
            statuses
                .iter()
                .map(|status| status.status)
                .collect::<Vec<_>>(),
            vec![
                HealthStatus::Healthy,
                HealthStatus::Disabled,
                HealthStatus::Unhealthy,
                HealthStatus::Unknown
            ]
        );
        assert_eq!(
            statuses[2].error.as_deref(),
            Some("executable not found: missing-mcp-test-binary")
        );
    }
}
