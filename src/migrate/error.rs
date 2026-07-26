//! Typed errors for state migration and validation.
//!
//! Migration must fail loudly (per AGENTS.md): never half-migrate config or
//! silently ignore corrupt artifacts that later would break pairing/events.

use thiserror::Error;

/// Errors returned by migration detection, backup, config conversion, and
/// state-tree validation.
#[derive(Debug, Error)]
pub enum MigrateError {
    /// Filesystem I/O failure (read, write, rename, create dir).
    #[error("migrate I/O error: {0}")]
    Io(#[from] std::io::Error),

    /// Go `config.json` is not valid UTF-8.
    #[error("legacy config.json is not valid UTF-8: {0}")]
    InvalidUtf8(String),

    /// Go `config.json` JSON deserialize failure.
    #[error("legacy config.json JSON error: {0}")]
    Json(#[from] serde_json::Error),

    /// TOML serialize/deserialize failure during migration or validation.
    #[error("config TOML error: {0}")]
    Toml(String),

    /// Config module failure after writing `config.toml` (reload/save path).
    #[error("config error during migration: {0}")]
    Config(#[from] crate::config::ConfigError),

    /// Event store open/query failure against a Go-created `SQLite` DB.
    #[error("event store validation failed: {0}")]
    EventStore(String),

    /// A required on-disk artifact exists but is structurally invalid.
    #[error("invalid state artifact {path}: {reason}")]
    InvalidArtifact {
        /// Relative or absolute path of the bad artifact.
        path: String,
        /// Human-readable reason the artifact failed validation.
        reason: String,
    },

    /// Migration cannot proceed because both Go and Rust config formats exist
    /// with conflicting content and no clear winner.
    #[error("ambiguous state: both config.json and config.toml present with conflicting content")]
    AmbiguousState,

    /// A migration step failed after a backup was taken; prior state was
    /// restored when possible.
    #[error("migration failed and prior state was restored: {0}")]
    RolledBack(String),

    /// A migration step failed and rollback itself also failed. Prior state
    /// may be inconsistent — operator must restore from the versioned backup.
    #[error("migration failed and rollback also failed: {error}; backup at {backup}")]
    RollbackFailed {
        /// Original migration error.
        error: String,
        /// Path of the versioned backup the operator can restore manually.
        backup: String,
    },
}
