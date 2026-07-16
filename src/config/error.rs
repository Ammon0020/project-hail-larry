//! Typed errors for the config module.
//!
//! Mirrors the ad-hoc `fmt.Errorf` wrapping in Go `internal/config` with a
//! `thiserror` enum so callers can match on the failure mode (e.g. missing
//! workspace vs. I/O error) instead of string-scanning.

use thiserror::Error;

/// Errors returned by config load/save/mutation operations.
#[derive(Debug, Error)]
pub enum ConfigError {
    /// Filesystem I/O failure (read, write, rename, fsync).
    #[error("config I/O error: {0}")]
    Io(#[from] std::io::Error),
    /// TOML parse failure on the on-disk config file.
    #[error("config TOML deserialize error: {0}")]
    TomlDeserialize(#[from] toml::de::Error),
    /// TOML serialization failure while saving.
    #[error("config TOML serialize error: {0}")]
    TomlSerialize(#[from] toml::ser::Error),
    /// JSON (DTO) serialization failure — used by the REST projection and the
    /// golden-fixture contract test.
    #[error("config JSON error: {0}")]
    Json(#[from] serde_json::Error),
    /// The user's home directory cannot be determined, so the default
    /// `~/.local-agent` state path is unavailable and `LOCAL_AGENT_STATE_DIR`
    /// is not set.
    #[error("cannot determine user home directory")]
    HomeDirNotFound,
    /// `remove_workspace` was called with a path that is not registered.
    #[error("workspace not registered: {0}")]
    WorkspaceNotRegistered(String),
    /// A caller-supplied input failed validation (e.g. empty workspace path).
    #[error("invalid config input: {0}")]
    InvalidInput(String),
    /// The on-disk config file is not valid UTF-8.
    #[error("config file is not valid UTF-8: {0}")]
    InvalidUtf8(String),
}
