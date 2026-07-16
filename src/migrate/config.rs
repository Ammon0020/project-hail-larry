//! Atomic Go `config.json` → Rust `config.toml` migration.
//!
//! Field names already match between Go JSON tags and Rust TOML (`camelCase`
//! via serde), so conversion is format-only (JSON parse → `Config` → TOML write).
//! Semantics:
//! - **Idempotent:** Rust-format state is a no-op; re-running after success leaves
//!   `config.toml` untouched and keeps the versioned backup.
//! - **Atomic / restart-safe:** backup first; write TOML via `Config::save`
//!   (`fsutil::atomic_write`); only after a successful reload remove (or leave)
//!   the JSON in a defined way. If interrupted before TOML exists, Go can still
//!   read `config.json`. If interrupted after TOML exists but before cleanup,
//!   detection reports `Both` and a second run is a validated no-op.
//! - **Failure-loud + rollback:** on any error after backup, restore the JSON
//!   from the versioned backup and remove any partial TOML.

use std::fs;
use std::path::{Path, PathBuf};

use tracing::{info, warn};

use crate::config::Config;
use crate::fsutil;

use super::detect::{
    config_json_backup_path, detect_format, StateFormat, GO_CONFIG_FILE, RUST_CONFIG_FILE,
};
use super::error::MigrateError;

/// File mode for config files (may contain paths that reveal user layout).
const CONFIG_FILE_PERM: u32 = 0o600;

/// Outcome of running config migration against a state directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigMigrationOutcome {
    /// No config files present — nothing to migrate.
    NoopEmpty,
    /// Already Rust-format (`config.toml` only, or validated dual state).
    NoopAlreadyRust,
    /// Successfully converted `config.json` → `config.toml` and wrote backup.
    Migrated {
        /// Path of the versioned JSON backup.
        backup: PathBuf,
    },
}

/// Run the config migration for `state_dir`.
///
/// Callers that set `LOCAL_AGENT_STATE_DIR` to `state_dir` (or pass an absolute
/// `state_dir` that matches how config will be loaded) get a consistent result:
/// after a successful `Migrated` outcome, `Config::load()` with that env var set
/// returns the migrated values.
///
/// # Errors
///
/// Returns [`MigrateError`] on I/O, parse, write, or ambiguous dual-state
/// problems. Failures after the backup is written attempt rollback.
pub fn migrate_config(state_dir: &Path) -> Result<ConfigMigrationOutcome, MigrateError> {
    match detect_format(state_dir) {
        StateFormat::Empty => Ok(ConfigMigrationOutcome::NoopEmpty),
        StateFormat::Rust => Ok(ConfigMigrationOutcome::NoopAlreadyRust),
        StateFormat::Both => validate_dual_state(state_dir),
        StateFormat::Go => migrate_go_to_rust(state_dir),
    }
}

/// Validate that dual-state (`config.json` + `config.toml`) is consistent enough
/// to treat as "already migrated" (idempotent second run / interrupted cleanup).
fn validate_dual_state(state_dir: &Path) -> Result<ConfigMigrationOutcome, MigrateError> {
    // TOML must load successfully via the same path the daemon uses.
    let toml_path = state_dir.join(RUST_CONFIG_FILE);
    let toml_bytes = fs::read(&toml_path)?;
    let toml_str =
        std::str::from_utf8(&toml_bytes).map_err(|e| MigrateError::InvalidUtf8(e.to_string()))?;
    let _toml_cfg: Config =
        toml::from_str(toml_str).map_err(|e| MigrateError::Toml(e.to_string()))?;

    // JSON must still parse so Go can read it if the user rolls back the binary.
    let json_path = state_dir.join(GO_CONFIG_FILE);
    let json_bytes = fs::read(&json_path)?;
    let json_str =
        std::str::from_utf8(&json_bytes).map_err(|e| MigrateError::InvalidUtf8(e.to_string()))?;
    let _: serde_json::Value = serde_json::from_str(json_str)?;

    // Prefer keeping both (Go readable + Rust primary). Do not delete JSON here;
    // operators may want to roll the Go binary back without restoring from backup.
    Ok(ConfigMigrationOutcome::NoopAlreadyRust)
}

/// Full migrate path when only `config.json` is present.
fn migrate_go_to_rust(state_dir: &Path) -> Result<ConfigMigrationOutcome, MigrateError> {
    let json_path = state_dir.join(GO_CONFIG_FILE);
    let toml_path = state_dir.join(RUST_CONFIG_FILE);
    let backup_path = config_json_backup_path(state_dir);

    // 1. Parse Go JSON into Config (field names match via camelCase serde).
    let json_bytes = fs::read(&json_path)?;
    let json_str =
        std::str::from_utf8(&json_bytes).map_err(|e| MigrateError::InvalidUtf8(e.to_string()))?;
    let mut cfg: Config = serde_json::from_str(json_str)?;

    // Ensure data_dir points at this state dir so Config::save writes config.toml
    // next to the original config.json (not wherever the JSON recorded previously).
    // Paths inside config may reference the original install location; we rewrite
    // the primary layout fields to the active state dir so isolated tests and
    // relocated state dirs work after migration.
    rewrite_layout_paths(&mut cfg, state_dir);

    // Apply the same legacy key defaulting as Config::load (TLS secure-by-default
    // and grace period) by looking at the raw JSON object for key presence.
    apply_legacy_defaults(&mut cfg, json_str)?;

    // 2. Versioned backup of the original JSON *before* any destructive change.
    //    If the backup already exists from a prior partial run, keep the first
    //    copy (original Go state) rather than overwriting with a possibly
    //    rewritten file.
    if !backup_path.is_file() {
        fsutil::atomic_write(&backup_path, &json_bytes, Some(CONFIG_FILE_PERM))?;
        info!(
            backup = %backup_path.display(),
            "created versioned backup of config.json before migration"
        );
    }

    // 3. Write config.toml atomically. On failure, restore JSON from backup
    //    (it should still be intact since we have not modified it yet) and
    //    remove any partial TOML.
    if let Err(e) = write_toml_config(&toml_path, &cfg) {
        // Attempt cleanup of partial TOML; JSON is still the Go-readable source.
        let _ = fs::remove_file(&toml_path);
        return Err(MigrateError::RolledBack(e.to_string()));
    }

    // 4. Verify the written TOML reloads cleanly (fail loud if corrupted).
    if let Err(e) = verify_toml_readable(&toml_path) {
        // Rollback: remove bad TOML so Go can keep using config.json.
        if let Err(rb) = fs::remove_file(&toml_path) {
            return Err(MigrateError::RollbackFailed {
                error: e.to_string(),
                backup: backup_path.display().to_string()
                    + &format!("; also failed to remove partial TOML: {rb}"),
            });
        }
        return Err(MigrateError::RolledBack(e.to_string()));
    }

    // 5. Leave config.json in place so Go remains readable if the operator
    //    switches binaries back. Dual-state is validated as NoopAlreadyRust
    //    on subsequent runs (idempotent). The versioned backup is the durable
    //    pre-migration snapshot.
    info!(
        state_dir = %state_dir.display(),
        "migrated config.json → config.toml"
    );

    Ok(ConfigMigrationOutcome::Migrated {
        backup: backup_path,
    })
}

/// Rewrite `dataDir` / `dbPath` / `tlsCertDir` to live under `state_dir`.
///
/// Go configs embed absolute paths for the install location. When the migration
/// runs against a relocated state dir (tests, `LOCAL_AGENT_STATE_DIR`), those
/// paths would point at the wrong tree. Anchoring them to `state_dir` matches
/// what `Config::default_or_error` would produce for the same override.
fn rewrite_layout_paths(cfg: &mut Config, state_dir: &Path) {
    let data_dir = state_dir.to_string_lossy().to_string();
    cfg.data_dir = data_dir;
    if cfg.db_path.is_empty()
        || Path::new(&cfg.db_path)
            .file_name()
            .is_some_and(|n| n == "local-agent.db")
    {
        // Keep the historical DB filename under the active state dir.
        cfg.db_path = state_dir
            .join("local-agent.db")
            .to_string_lossy()
            .to_string();
    }
    if cfg.tls_cert_dir.is_empty()
        || Path::new(&cfg.tls_cert_dir)
            .file_name()
            .is_some_and(|n| n == "tls")
    {
        cfg.tls_cert_dir = state_dir.join("tls").to_string_lossy().to_string();
    }
}

/// Mirror `Config::load` legacy key presence checks using a raw JSON map.
fn apply_legacy_defaults(cfg: &mut Config, json_str: &str) -> Result<(), MigrateError> {
    let raw: serde_json::Map<String, serde_json::Value> = serde_json::from_str(json_str)?;

    // Secure-by-default: missing tlsEnabled → force true (explicit false stands).
    if !raw.contains_key("tlsEnabled") {
        cfg.tls_enabled = true;
    }
    // Missing revocationGracePeriodSeconds → 300 (explicit 0 stands).
    if !raw.contains_key("revocationGracePeriodSeconds") {
        cfg.revocation_grace_period_seconds = 300;
    }
    // Scalar zero-fills for required runtime fields.
    if cfg.port == 0 {
        cfg.port = 7337;
    }
    if cfg.host.is_empty() {
        cfg.host = "0.0.0.0".to_string();
    }
    if cfg.pairing_ttl_seconds == 0 {
        cfg.pairing_ttl_seconds = 300;
    }
    Ok(())
}

/// Serialize `cfg` to TOML and write atomically at `toml_path`.
fn write_toml_config(toml_path: &Path, cfg: &Config) -> Result<(), MigrateError> {
    let toml_str = toml::to_string_pretty(cfg).map_err(|e| MigrateError::Toml(e.to_string()))?;
    fsutil::atomic_write(toml_path, toml_str.as_bytes(), Some(CONFIG_FILE_PERM))?;
    Ok(())
}

/// Ensure a just-written TOML config parses back into `Config`.
fn verify_toml_readable(toml_path: &Path) -> Result<(), MigrateError> {
    let data = fs::read(toml_path)?;
    let s = std::str::from_utf8(&data).map_err(|e| MigrateError::InvalidUtf8(e.to_string()))?;
    let _: Config = toml::from_str(s).map_err(|e| MigrateError::Toml(e.to_string()))?;
    Ok(())
}

/// Restore `config.json` from a versioned backup and remove `config.toml`.
///
/// Used by tests (and by operators/tools) to roll back a migration. Logs a
/// warning if the backup is missing.
pub fn restore_config_from_backup(state_dir: &Path) -> Result<(), MigrateError> {
    let backup = config_json_backup_path(state_dir);
    if !backup.is_file() {
        warn!(
            backup = %backup.display(),
            "no versioned config.json backup to restore"
        );
        return Err(MigrateError::InvalidArtifact {
            path: backup.display().to_string(),
            reason: "versioned backup not found".into(),
        });
    }
    let bytes = fs::read(&backup)?;
    let json_path = state_dir.join(GO_CONFIG_FILE);
    fsutil::atomic_write(&json_path, &bytes, Some(CONFIG_FILE_PERM))?;
    let toml_path = state_dir.join(RUST_CONFIG_FILE);
    if toml_path.exists() {
        fs::remove_file(&toml_path)?;
    }
    Ok(())
}
