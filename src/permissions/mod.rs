//! Permission request/response and policies (Go `internal/permissions/`).
//!
//! Receives `session/request_permission` prompts from agents, presents them to
//! paired devices via the injected [`PermissionSink`], enforces
//! `allow_always` / `allow_session` / `reject_always` policies, auto-denies
//! stale prompts, and records an ephemeral audit log for the running daemon.
//!
//! Layout:
//! - [`sink`] — narrow notification sink (replaces Go's `SetCallback`)
//! - [`manager`] — [`Manager`] + [`AuditEntry`] + stale sweeper
//!
//! Durable policy / audit storage is post-parity work; the in-memory maps are
//! the initial port. See `docs/plans/rust-port/complete-S-PERMISSIONS-permissions-med.md`.

mod manager;
mod sink;

#[cfg(test)]
mod tests;

pub use manager::{
    AuditEntry, Manager, DEFAULT_STALE_TIMEOUT, DEFAULT_SWEEP_INTERVAL, MAX_AUDIT_ENTRIES,
};
pub use sink::{null_sink, EventBusPermissionSink, NullSink, PermissionSink};

// Re-export the trait so callers can `use crate::permissions::PermissionManager`.
pub use crate::interfaces::PermissionManager;
