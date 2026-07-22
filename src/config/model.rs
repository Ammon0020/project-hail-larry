//! `Config` struct, agent DTOs, defaults, and in-memory mutation methods.
//!
//! Mirrors Go `internal/config/config.go` `Config` + `interfaces.AgentInfo`/
//! `AgentModel`. The on-disk format is TOML in the Rust port (replacing Go's
//! `encoding/json`/`config.json` — see S-MIGRATE for the JSON→TOML migration);
//! the TOML field names match Go's JSON tags exactly (`camelCase`) so a
//! migrated file carries the same keys. The same `serde` rename also drives
//! the JSON DTO projection consumed by the REST API and the S-CONTRACT golden
//! fixture (`tests/contract/golden/dto/config_default.json`).
//!
//! Unknown (forward-compatible) TOML keys are captured into `extra` via
//! `#[serde(flatten)]` and re-emitted on save, so a newer daemon writing an
//! unknown field does not silently lose it on the next round-trip — matching
//! the Go `pelletier/go-toml` decoder behavior of preserving unknown keys.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use super::error::ConfigError;
use super::{DEFAULT_CREDENTIAL_INACTIVITY_TTL_SECONDS, DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS};

/// `AgentModel` describes a single model offered by a registered agent.
///
/// Mirrors Go `interfaces.AgentModel` (`json:"id"`, `json:"name"`). Optional
/// fields are filled by harness autodetection when the agent advertises them
/// (e.g. Devin `configOptions.currentValue` → `preferred`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentModel {
    pub id: String,
    pub name: String,
    /// True when this is the agent's current/default model at detect time.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub preferred: bool,
    /// Agent-advertised image support (e.g. Devin `_meta.supportsImages`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub supports_images: Option<bool>,
    /// Optional short description from the agent (cost is not exposed by Devin).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl AgentModel {
    /// Construct a model with only id/name (optional metadata left unset).
    #[must_use]
    pub fn new(id: String, name: String) -> Self {
        Self {
            id,
            name,
            ..Self::default()
        }
    }
}

/// `AgentInfo` describes a registered agent harness persisted in the config.
///
/// Mirrors Go `interfaces.AgentInfo`. `args` and `warning` are `omitempty` in
/// Go and use `skip_serializing_if` here so the serialized shape matches.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentInfo {
    pub id: String,
    pub name: String,
    pub command: String,
    /// CLI args passed after the command. Omitted from output when empty
    /// (Go `args,omitempty`).
    #[serde(skip_serializing_if = "Vec::is_empty", default)]
    pub args: Vec<String>,
    pub models: Vec<AgentModel>,
    /// Agent health warning string (e.g. "Executable not found in PATH").
    /// Omitted from output when empty (Go `warning,omitempty`).
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub warning: String,
}

/// `Config` is the persistent application configuration stored at
/// `<state_dir>/config.toml` where `<state_dir>` is `$LOCAL_AGENT_STATE_DIR`
/// or `~/.local-agent`.
///
/// Field order matches the S-CONTRACT golden DTO so `serde_json` emits keys in
/// the same byte-stable order as the Go daemon. `skip_serializing_if` mirrors
/// Go's `omitempty` tags exactly: `tlsCertDir` (empty), `httpsPort` (0),
/// `pairingTtlSeconds` (0), `credentialInactivityTtlSeconds` (0),
/// `allowRemoteWorkspaceRegistration` (false), `revocationGracePeriodSeconds`
/// (0). `extra` captures any unknown TOML keys for forward-compatible
/// round-tripping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    // All fields default on omission so a partial TOML file (e.g. a legacy
    // config predating a field) zero-fills and is then topped up from
    // `default_or_error` in `load`, mirroring Go's `json.Unmarshal` behavior.
    #[serde(default)]
    pub port: i64,
    #[serde(default)]
    pub host: String,
    #[serde(default)]
    pub data_dir: String,
    #[serde(default)]
    pub db_path: String,
    #[serde(default)]
    pub workspaces: Vec<String>,
    #[serde(default)]
    pub agents: Vec<AgentInfo>,
    #[serde(default)]
    pub tls_enabled: bool,
    /// Directory holding the self-signed TLS cert/key. Omitted when empty.
    #[serde(skip_serializing_if = "String::is_empty", default)]
    pub tls_cert_dir: String,
    /// HTTPS listener port; 0 means `port + 1` at runtime. Omitted when 0.
    #[serde(skip_serializing_if = "is_zero_i64", default)]
    pub https_port: i64,
    /// Pairing session TTL in seconds. Omitted when 0.
    #[serde(skip_serializing_if = "is_zero_i64", default)]
    pub pairing_ttl_seconds: i64,
    /// Sliding-window inactivity expiry for paired device credentials. An
    /// explicit 0 disables expiry; omitted means "legacy/disabled" on load.
    #[serde(skip_serializing_if = "is_zero_i64", default)]
    pub credential_inactivity_ttl_seconds: i64,
    /// Whether paired devices may register new workspaces remotely. Omitted
    /// when false.
    #[serde(skip_serializing_if = "std::ops::Not::not", default)]
    pub allow_remote_workspace_registration: bool,
    /// Grace period (seconds) for device revocation / remote workspace
    /// registration pending actions. Explicit 0 = instant execution; omitted
    /// means "legacy" on load and is defaulted to 300.
    #[serde(skip_serializing_if = "is_zero_i64", default)]
    pub revocation_grace_period_seconds: i64,
    /// Forward-compatible unknown TOML keys, preserved across round-trips.
    #[serde(flatten)]
    #[serde(default)]
    pub extra: toml::Table,
}

/// `skip_serializing_if` helper mirroring Go's `omitempty` for integer fields
/// (drop on zero).
fn is_zero_i64(v: &i64) -> bool {
    *v == 0
}

impl Config {
    /// `default_or_error` returns the default configuration, deriving
    /// `dataDir`/`dbPath`/`tlsCertDir` from the resolved state directory.
    ///
    /// Errors only when the state directory cannot be resolved (no home dir
    /// and `LOCAL_AGENT_STATE_DIR` unset). Mirrors Go `DefaultOrError`.
    pub fn default_or_error() -> Result<Self, ConfigError> {
        let state_dir = Self::resolved_state_dir()?;
        let data_dir = state_dir.to_string_lossy().to_string();
        let db_path = state_dir
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
        let tls_cert_dir = state_dir.join("tls").to_string_lossy().to_string();
        Ok(Self {
            port: 7337,
            host: "0.0.0.0".to_string(),
            data_dir,
            db_path,
            workspaces: Vec::new(),
            agents: Vec::new(),
            // Secure by default: device Bearer tokens must not travel in
            // cleartext over the LAN. An existing config that explicitly sets
            // `tlsEnabled = false` is respected by `load`.
            tls_enabled: true,
            tls_cert_dir,
            https_port: 0,
            pairing_ttl_seconds: 300,
            credential_inactivity_ttl_seconds: DEFAULT_CREDENTIAL_INACTIVITY_TTL_SECONDS,
            allow_remote_workspace_registration: false,
            revocation_grace_period_seconds: DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS,
            extra: toml::Table::new(),
        })
    }

    /// `resolved_state_dir` returns the state directory the config should use.
    ///
    /// When `LOCAL_AGENT_STATE_DIR` is set and non-empty its value is used
    /// verbatim; otherwise the default `~/.local-agent` path is derived from
    /// the user's home directory. This is the single override point consulted
    /// by both `default_or_error` and `load` so the CLI and daemon agree on
    /// the state directory (mirrors Go `resolvedStateDir`).
    pub fn resolved_state_dir() -> Result<PathBuf, ConfigError> {
        if let Some(dir) = std::env::var_os(super::STATE_DIR_ENV_VAR) {
            if !dir.is_empty() {
                return Ok(PathBuf::from(dir));
            }
        }
        let home = crate::fsutil::home_dir().ok_or(ConfigError::HomeDirNotFound)?;
        Ok(home.join(".local-agent"))
    }

    // ---- Workspace mutation methods ----------------------------------------

    /// `add_workspace` registers an absolute workspace path and persists the
    /// config. Duplicate paths are silently kept unique (no-op if already
    /// registered). Mirrors the host-CLI `app add-folder` persistence path.
    pub fn add_workspace(&mut self, abs_path: &str) -> Result<(), ConfigError> {
        if abs_path.is_empty() {
            return Err(ConfigError::InvalidInput(
                "workspace path is empty".to_string(),
            ));
        }
        if !self.workspaces.iter().any(|w| w == abs_path) {
            self.workspaces.push(abs_path.to_string());
        }
        self.save()
    }

    /// `remove_workspace` drops the given absolute path from the workspaces
    /// list and persists. Errors if the path was not registered. Mirrors Go
    /// `RemoveWorkspacePath`.
    pub fn remove_workspace(&mut self, abs_path: &str) -> Result<(), ConfigError> {
        let before = self.workspaces.len();
        self.workspaces.retain(|w| w != abs_path);
        if self.workspaces.len() == before {
            return Err(ConfigError::WorkspaceNotRegistered(abs_path.to_string()));
        }
        self.save()
    }

    /// `list_workspaces` returns a copy of the registered workspace paths.
    pub fn list_workspaces(&self) -> Vec<String> {
        self.workspaces.clone()
    }

    // ---- Agent mutation methods --------------------------------------------

    /// `upsert_agent` adds or replaces an agent by ID and persists. Mirrors Go
    /// `UpsertAgent`.
    pub fn upsert_agent(&mut self, agent: AgentInfo) -> Result<(), ConfigError> {
        if let Some(slot) = self.agents.iter_mut().find(|a| a.id == agent.id) {
            *slot = agent;
        } else {
            self.agents.push(agent);
        }
        self.save()
    }

    /// `delete_agent` removes an agent by ID and persists. No error if the ID
    /// is not present (mirrors Go `DeleteAgent`).
    pub fn delete_agent(&mut self, id: &str) -> Result<(), ConfigError> {
        self.agents.retain(|a| a.id != id);
        self.save()
    }
}

/// `Default` for `Config` mirrors Go `Default()`.
///
/// Go panics when the home directory cannot be determined; the Rust port has a
/// crate-wide no-panic policy (the daemon serves the LAN and must not crash on
/// a transient env failure), so this implementation logs the error via
/// `tracing::error!` and falls back to a relative `.local-agent` data
/// directory. Callers that can surface an error should use
/// [`Config::default_or_error`] instead.
impl std::default::Default for Config {
    fn default() -> Self {
        match Self::default_or_error() {
            Ok(c) => c,
            Err(e) => {
                tracing::error!(
                    error = %e,
                    "cannot resolve state dir for default config; \
                     falling back to relative .local-agent"
                );
                let fallback = PathBuf::from(".local-agent");
                Self {
                    port: 7337,
                    host: "0.0.0.0".to_string(),
                    db_path: fallback
                        .join("local-agent.db")
                        .to_string_lossy()
                        .to_string(),
                    tls_cert_dir: fallback.join("tls").to_string_lossy().to_string(),
                    data_dir: fallback.to_string_lossy().to_string(),
                    workspaces: Vec::new(),
                    agents: Vec::new(),
                    tls_enabled: true,
                    https_port: 0,
                    pairing_ttl_seconds: 300,
                    credential_inactivity_ttl_seconds: DEFAULT_CREDENTIAL_INACTIVITY_TTL_SECONDS,
                    allow_remote_workspace_registration: false,
                    revocation_grace_period_seconds: DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS,
                    extra: toml::Table::new(),
                }
            }
        }
    }
}
