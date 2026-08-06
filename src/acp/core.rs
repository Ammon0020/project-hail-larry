//! ACP session client facade and internal lifecycle services.
//!
//! The public client is intentionally small: transport ownership stays in the
//! actor, while lifecycle and request operations remain private implementation
//! details of this module.

mod actor;
mod client;
mod diagnostics;
mod events;
mod handlers;
pub(super) mod lifecycle;
mod mcp;
mod operations;
mod registry;

pub use client::{Client, ClientDeps};
pub use registry::SessionState;

/// Maximum retained agent stderr diagnostic tail. Agent stderr is untrusted and
/// must never be allowed to grow the daemon's memory without bound.
pub const STDERR_TAIL_BYTES: usize = 8 * 1024;

/// Maximum concurrent live ACP sessions. Each session pins an agent child
/// process, so a cap prevents unbounded process-exhaustion `DoS`.
pub(super) const MAX_SESSIONS: usize = 32;
/// Safe default for a model-switch rebind transfer (256 KiB).
pub(super) const MODEL_SWITCH_TRANSFER_BYTES: i64 = 256 * 1024;
