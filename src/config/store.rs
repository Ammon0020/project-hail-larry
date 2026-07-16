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
//! `save` writes atomically: temp file in the same directory → `fsync` file →
//! `chmod 0600` → `rename` → `fsync` parent directory (best-effort). A crash
//! at any point leaves either the previous file intact or a temp file that is
//! never read, never a half-written `config.toml`. Mirrors Go
//! `mcp.WriteFileAtomic`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock, RwLockReadGuard, RwLockWriteGuard};

use super::error::ConfigError;
use super::model::Config;
use super::{CONFIG_FILE_NAME, CONFIG_FILE_PERM, DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS};

/// Monotonic counter used to derive unique temp-file names per process so
/// concurrent writers in the same directory never collide.
static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

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

    /// `save` writes the config to `<data_dir>/config.toml` atomically with
    /// mode `0600`. The data directory is created if missing. Mirrors Go
    /// `Save` → `mcp.WriteFileAtomic`.
    pub fn save(&self) -> Result<(), ConfigError> {
        let toml_str = toml::to_string_pretty(self)?;
        let config_path = Path::new(&self.data_dir).join(CONFIG_FILE_NAME);
        atomic_write_file(&config_path, toml_str.as_bytes(), CONFIG_FILE_PERM)?;
        Ok(())
    }
}

/// `atomic_write_file` writes `data` to `path` atomically: a temp file in the
/// same directory is written, `fsync`'d, `chmod`'d to `perm`, then renamed over
/// the target, and the parent directory is `fsync`'d best-effort. A crash
/// leaves either the old file or a temp file (never read), never a truncated
/// target. The temp file lives in the same directory so the rename is on the
/// same filesystem. Mirrors Go `mcp.WriteFileAtomic`.
fn atomic_write_file(path: &Path, data: &[u8], perm: u32) -> Result<(), std::io::Error> {
    #[cfg(not(unix))]
    let _ = perm;

    let dir = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(dir)?;

    let basename = path
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "config".to_string());
    let pid = std::process::id();
    let c = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
    // Leading dot + `.tmp` suffix matches Go's `.<base>.*.tmp` pattern so
    // concurrent writers for different files in the same dir never collide.
    let tmp_name = format!(".{basename}.{pid}.{c}.tmp");
    let tmp_path = dir.join(&tmp_name);

    // Write + fsync the temp file. On any failure, clean up the temp file so
    // a later writer does not trip over our partial output.
    let mut tmp = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp_path)?;
    let write_res: Result<(), std::io::Error> = (|| {
        tmp.write_all(data)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            tmp.set_permissions(fs::Permissions::from_mode(perm))?;
        }
        // Flush contents + metadata to stable storage before the rename so a
        // power loss cannot leave a renamed-but-empty file.
        tmp.sync_all()?;
        Ok(())
    })();
    drop(tmp);
    if let Err(e) = write_res {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    fs::rename(&tmp_path, path)?;

    // Best-effort directory sync so the new dirent is durable. Some platforms
    // (notably Windows) reject Sync on directories; ignore those errors.
    #[cfg(unix)]
    {
        if let Ok(d) = File::open(dir) {
            let _ = d.sync_all();
        }
    }
    Ok(())
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

    /// `save` persists the current config under the read lock (save only
    /// reads fields to serialize).
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
