//! Config storage in `~/.local-agent/` (Go `internal/config/`).
//!
//! TOML config persistence with atomic, durable writes. The on-disk format is
//! TOML in the Rust port (replacing Go's `config.json`); the TOML field names
//! match Go's JSON tags exactly so a migrated file carries the same keys, and
//! the S-CONTRACT golden DTO (`tests/contract/golden/dto/config_default.json`)
//! is reproduced via the same `serde` rename projected to JSON.
//!
//! Layout:
//! - [`error`]  — `ConfigError` enum (thiserror).
//! - [`model`]  — `Config`, `AgentInfo`, `AgentModel`, defaults, in-memory
//!   workspace/agent mutation methods.
//! - [`store`]  — `load`, atomic `save`, and the thread-safe `ConfigStore`
//!   wrapper (`RwLock<Config>`).
//!
//! See `docs/plans/rust-port/complete-S-CONFIG-config-med.md`.

mod error;
mod model;
mod store;
#[cfg(test)]
mod tests;

pub use error::ConfigError;
pub use model::{AgentInfo, AgentModel, Config};
pub use store::ConfigStore;

/// Environment variable that, when set, overrides the default `~/.local-agent`
/// state directory. Used by the S-CONTRACT fixture harness to run the daemon
/// and CLI against an isolated state directory without touching the user's
/// real config. Mirrors Go `stateDirEnvVar`.
pub const STATE_DIR_ENV_VAR: &str = "LOCAL_AGENT_STATE_DIR";

/// On-disk config file name (TOML in the Rust port).
const CONFIG_FILE_NAME: &str = "config.toml";

/// Config file permissions: the file may contain secrets, so `0600`.
/// Mirrors Go `configFilePerm`.
const CONFIG_FILE_PERM: u32 = 0o600;

/// Default sliding-window credential inactivity expiry (30 days). Applied to
/// fresh configs so sliding expiry is on by default; an explicit `0` in an
/// existing config disables expiry and is respected. Mirrors Go
/// `defaultCredentialInactivityTTLSeconds`.
const DEFAULT_CREDENTIAL_INACTIVITY_TTL_SECONDS: i64 = 2_592_000;

/// Default grace period (5 minutes) for device revocation / remote workspace
/// registration pending actions. Applied to fresh configs and to legacy config
/// files omitting the key; an explicit `0` disables the grace period (instant
/// execution) and is respected. Mirrors Go
/// `defaultRevocationGracePeriodSeconds`.
const DEFAULT_REVOCATION_GRACE_PERIOD_SECONDS: i64 = 300;
