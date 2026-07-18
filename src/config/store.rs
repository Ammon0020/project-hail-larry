//! Config persistence: `load`, atomic `save`, and the thread-safe
//! `ConfigStore` wrapper.
//!
//! `load` reads `<state_dir>/config.toml` (TOML in the Rust port; Go wrote
//! `config.json` — see S-MIGRATE). Missing file → `default_or_error`. The
//! legacy-key defaulting for `tlsEnabled` and `revocationGracePeriodSeconds`
//! mirrors Go's raw-map key-presence check: a missing bool/int zero-fills to
//! a default that we cannot otherwise distinguish from an explicit opt-out, so
//! we parse the raw `toml::Table` first and force the secure-by-default /
//! 5-minute-grace value when the key is absent.
//!
//! `save` writes via [`crate::fsutil::atomic_write`] (temp + fsync + chmod +
//! rename + parent fsync). A crash leaves either the previous file intact or a
//! temp that is never read, never a half-written `config.toml`. Mirrors Go
//! `mcp.WriteFileAtomic`.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::error::ConfigError;
use super::model::Config;
use super::{CONFIG_FILE_NAME, CONFIG_FILE_PERM, DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS};
use crate::fsutil;

impl Config {
    /// `load` reads the config from `<state_dir>/config.toml`, where
    /// `<state_dir>` is `LOCAL_AGENT_STATE_DIR` when set, otherwise
    /// `~/.local-agent`. Returns the default config when the file does not
    /// exist. Mirrors Go `Load`.
    pub fn load() -> Result<Self, ConfigError> {
        let state_dir = Self::resolved_state_dir()?;
        let config_path = state_dir.join(CONFIG_FILE_NAME);

        let data = match fs::read(&config_path) {
            Ok(d) => d,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // No config file yet — return defaults. The state dir was
                // resolved successfully above, so default_or_error cannot
                // fail here.
                return Self::default_or_error();
            }
            Err(e) => return Err(e.into()),
        };

        let raw_str =
            std::str::from_utf8(&data).map_err(|e| ConfigError::InvalidUtf8(e.to_string()))?;

        // Parse the raw table first so we can detect key presence for the
        // legacy-defaulting fields (tlsEnabled, revocationGracePeriodSeconds).
        let table: toml::Table = toml::from_str(raw_str)?;
        let mut cfg: Self = toml::from_str(raw_str)?;

        // Secure-by-default upgrade: an older config file predating
        // `tlsEnabled` loads as `false` (bool zero-fill). Force TLS on unless
        // the key is explicitly present (including an explicit `false`).
        if !table.contains_key("tlsEnabled") {
            cfg.tls_enabled = true;
        }
        // 5-minute grace window by default on upgrade: a legacy file omitting
        // `revocationGracePeriodSeconds` loads as 0 (int zero-fill), which we
        // cannot distinguish from an explicit 0 (instant execution). Default
        // to 300 only when the key is absent; an explicit 0 stands.
        if !table.contains_key("revocationGracePeriodSeconds") {
            cfg.revocation_grace_period_seconds = DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS;
        }

        // Fill in any missing scalar defaults from a fresh default config.
        let def = Self::default_or_error()?;
        if cfg.port == 0 {
            cfg.port = def.port;
        }
        if cfg.host.is_empty() {
            cfg.host = def.host;
        }
        if cfg.data_dir.is_empty() {
            cfg.data_dir = def.data_dir;
        }
        if cfg.db_path.is_empty() {
            cfg.db_path = def.db_path;
        }
        if cfg.tls_cert_dir.is_empty() {
            cfg.tls_cert_dir = def.tls_cert_dir;
        }
        if cfg.pairing_ttl_seconds == 0 {
            cfg.pairing_ttl_seconds = def.pairing_ttl_seconds;
        }
        // Note: credential_inactivity_ttl_seconds is intentionally NOT
        // zero-filled. With a plain int we cannot distinguish "omitted" from
        // "explicit 0" (which disables expiry), and silently re-enabling
        // 30-day expiry on an existing install would surprise users who relied
        // on permanent credentials. A fresh install gets the default via
        // default_or_error; an existing file omitting the field loads as 0.
        // Vec fields (workspaces, agents) default to empty via serde and need
        // no explicit reset.

        Ok(cfg)
    }

    /// `save` writes the config to the active state directory atomically with
    /// mode `0600`. The state directory is created if missing. Mirrors Go
    /// `Save` → `mcp.WriteFileAtomic`.
    ///
    /// # State-dir mismatch guard
    ///
    /// `save` always targets [`Self::resolved_state_dir`] (env override or
    /// `~/.local-agent`), **not** the persisted `data_dir` field. If `data_dir`
    /// points under the process temp directory while the active state dir does
    /// not, this refuses to write — otherwise a daemon/unit-test config with
    /// ephemeral paths can poison the real user config (see known-issues).
    pub fn save(&self) -> Result<(), ConfigError> {
        let state_dir = Self::resolved_state_dir()?;
        if let Some(data_dir) = non_empty_path(&self.data_dir) {
            if is_dangerous_temp_data_dir(data_dir, &state_dir) {
                return Err(ConfigError::StateDirMismatch {
                    data_dir: data_dir.display().to_string(),
                    state_dir: state_dir.display().to_string(),
                });
            }
        }
        let toml_str = toml::to_string_pretty(self)?;
        let config_path = state_dir.join(CONFIG_FILE_NAME);
        fsutil::atomic_write(&config_path, toml_str.as_bytes(), Some(CONFIG_FILE_PERM))?;
        Ok(())
    }
}

/// True when `data_dir` is under the process temp dir but `state_dir` is not.
///
/// Both-under-temp is allowed (contract harness + unit tests). Equal paths are
/// never dangerous. Legacy installs where `data_dir` and the active state dir
/// diverge outside temp (moved install) are still allowed.
fn is_dangerous_temp_data_dir(data_dir: &Path, state_dir: &Path) -> bool {
    if paths_equal_loose(data_dir, state_dir) {
        return false;
    }
    let tmp = std::env::temp_dir();
    path_is_under(data_dir, &tmp) && !path_is_under(state_dir, &tmp)
}

fn non_empty_path(s: &str) -> Option<&Path> {
    if s.is_empty() {
        None
    } else {
        Some(Path::new(s))
    }
}

/// Prefix check with a best-effort absolute form (no canonicalize — paths may
/// not exist yet when `save` creates the state dir).
fn path_is_under(path: &Path, root: &Path) -> bool {
    let path = absolute_approx(path);
    let root = absolute_approx(root);
    path.starts_with(&root)
}

fn paths_equal_loose(a: &Path, b: &Path) -> bool {
    a == b || absolute_approx(a) == absolute_approx(b)
}

fn absolute_approx(path: &Path) -> PathBuf {
    if path.is_absolute() {
        return path.to_path_buf();
    }
    std::env::current_dir()
        .map(|cwd| cwd.join(path))
        .unwrap_or_else(|_| path.to_path_buf())
}

/// `ConfigStore` wraps a `Config` in an `RwLock` for thread-safe shared access
/// from the daemon's HTTP handlers. Replaces Go's `sync.Mutex` embedded in the
/// `Config` struct. Read-heavy access (every request reads config) takes the
/// read lock; mutations (workspace add/remove, agent upsert/delete) take the
/// write lock and persist under the lock so concurrent writers cannot
/// interleave on-disk writes.
#[derive(Debug, Clone)]
pub struct ConfigStore {
    cfg: Arc<RwLock<Config>>,
}

impl ConfigStore {
    /// `new` wraps an existing `Config`.
    pub fn new(cfg: Config) -> Self {
        Self {
            cfg: Arc::new(RwLock::new(cfg)),
        }
    }

    /// `load` reads the config from disk and wraps it. Convenience for
    /// `ConfigStore::new(Config::load()?)`.
    pub fn load() -> Result<Self, ConfigError> {
        Ok(Self::new(Config::load()?))
    }

    /// `read` returns a read guard on the wrapped config. A poisoned lock
    /// (a writer panicked while holding it) is recovered via `into_inner`
    /// rather than propagated as a panic, preserving the daemon's no-panic
    /// guarantee.
    pub fn read(&self) -> RwLockReadGuard<'_, Config> {
        self.cfg.read().unwrap_or_else(|e| e.into_inner())
    }

    /// `write` returns a write guard on the wrapped config. Same poison
    /// recovery as `read`.
    pub fn write(&self) -> RwLockWriteGuard<'_, Config> {
        self.cfg.write().unwrap_or_else(|e| e.into_inner())
    }

    /// `save` persists the current config while holding its read lock.
    ///
    /// This intentionally blocks mutations during serialization and fsync. A
    /// caller can mutate and save through [`Self::write`], so releasing the
    /// read lock before persistence could let an older snapshot overwrite that
    /// later direct save.
    pub fn save(&self) -> Result<(), ConfigError> {
        self.read().save()
    }

    /// `into_inner` unwraps the store, returning the inner `Config`. Clones
    /// are dropped; the underlying `Arc` is consumed only if this is the last
    /// reference (otherwise returns a clone).
    pub fn into_inner(self) -> Config {
        // `Arc::try_unwrap` fails when shared; fall back to cloning so callers
        // always get a value without panicking.
        match Arc::try_unwrap(self.cfg) {
            Ok(lock) => lock.into_inner().unwrap_or_else(|e| e.into_inner()),
            Err(arc) => arc.read().unwrap_or_else(|e| e.into_inner()).clone(),
        }
    }
}
