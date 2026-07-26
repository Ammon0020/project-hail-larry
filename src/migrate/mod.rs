//! Existing-state compatibility and migration (S-MIGRATE).
//!
//! Allows the Rust binary to replace the Go binary without losing user state.
//! The only *format-transforming* migration today is config:
//! `config.json` (Go/JSON) → `config.toml` (Rust/TOML). Other artifacts
//! (`SQLite` event DB, `devices.json`, `conversations.json`, `mcp.json`,
//! uploads, TLS dir) keep their Go on-disk formats and are validated for
//! openability; semantic load for pairing/MCP/ACP/uploads is deferred to those
//! module ports.
//!
//! Layout:
//! - [`error`]    — [`MigrateError`]
//! - [`detect`]   — format detection + versioned backup naming
//! - [`config`]   — atomic config.json → config.toml migration
//! - [`validate`] — open/validate non-config Go state artifacts
//!
//! Invariants (per story + AGENTS.md):
//! - atomic (backup before destructive change; TOML via atomic write)
//! - idempotent (second run is no-op)
//! - restart-safe (interrupted leave prior Go-readable or completed dual state)
//! - failure-loud (errors returned, no silent success)

mod config;
mod detect;
mod error;
mod validate;

#[cfg(test)]
mod tests;

pub use config::{migrate_config, restore_config_from_backup, ConfigMigrationOutcome};
pub use detect::{
    config_json_backup_path, detect_format, StateFormat, GO_CONFIG_FILE, MIGRATE_FORMAT_VERSION,
    RUST_CONFIG_FILE,
};
pub use error::MigrateError;
pub use validate::{
    validate_event_db_async, validate_state_tree, ArtifactStatus, StateValidation,
    CONVERSATIONS_FILE, DEVICES_FILE, EVENT_DB_FILE, MCP_FILE, TLS_DIR, UPLOADS_DIR,
};

use std::path::Path;

use tracing::info;

/// Full report from [`run_migrations`].
#[derive(Debug, Clone)]
pub struct MigrationReport {
    /// Detected format before migration ran.
    pub before: StateFormat,
    /// Detected format after migration ran.
    pub after: StateFormat,
    /// Config migration outcome.
    pub config: ConfigMigrationOutcome,
    /// Validation of the rest of the state tree.
    pub validation: StateValidation,
}

impl MigrationReport {
    /// True when config migration (if any) succeeded and validation has no hard
    /// failures.
    #[must_use]
    pub fn is_ok(&self) -> bool {
        self.validation.is_ok()
    }
}

/// Run all startup migrations + state validation against `state_dir`.
///
/// Intended call site: daemon start, **before** `Config::load` for a state dir
/// that may still be Go-format. After success, `config.toml` is present when
/// there was anything to migrate.
///
/// # Errors
///
/// - Config migration I/O / parse / rollback failures ([`MigrateError`])
/// - Event DB present but unreadable (schema/payload drift)
/// - Any hard validation failure in required artifacts that exist on disk
pub fn run_migrations(state_dir: &Path) -> Result<MigrationReport, MigrateError> {
    let before = detect_format(state_dir);
    info!(
        state_dir = %state_dir.display(),
        ?before,
        "starting state migration / validation"
    );

    let config = migrate_config(state_dir)?;
    let after = detect_format(state_dir);

    let mut validation = validate_state_tree(state_dir)?;
    // If validation found hard failures on existing artifacts, fail loudly.
    if !validation.failed.is_empty() {
        let detail = validation
            .failed
            .iter()
            .map(|f| format!("{}: {}", f.name, f.detail))
            .collect::<Vec<_>>()
            .join("; ");
        return Err(MigrateError::InvalidArtifact {
            path: state_dir.display().to_string(),
            reason: detail,
        });
    }

    validation
        .notes
        .push(format!("config migration: {config:?}"));
    validation
        .notes
        .push(format!("format before={before:?} after={after:?}"));

    Ok(MigrationReport {
        before,
        after,
        config,
        validation,
    })
}
