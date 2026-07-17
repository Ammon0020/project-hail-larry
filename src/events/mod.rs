//! SQLite event store (Go `internal/events/`).
//!
//! WAL-mode, append-only event log with retention pruning, query, and replay.
//! Uses `rusqlite` (bundled) behind a blocking boundary: a single connection is
//! owned by [`Store`] and async callers enter only via `spawn_blocking`, so a
//! DB lock is never held across `.await`.
//!
//! Layout:
//! - [`payload`] — on-disk JSON payload matching Go `eventPayload`
//! - [`store`] — [`Store`] + [`EventStore`] impl, prune, PRAGMAs
//! - [`publisher`] — [`EventBus`] (`EventStore` + `EventPublisher` + subscribe
//!   handoff: subscribe → replay → dedupe by ID → live delivery)
//!
//! See `docs/plans/rust-port/stories/S-EVENTS-event-store.md`.

mod payload;
mod publisher;
mod store;

#[cfg(test)]
mod tests;

pub use publisher::{EventBus, EventSubscription, SharedEventBus, SubRecv};
pub use store::{Store, DEFAULT_PRUNE_INTERVAL, DEFAULT_PRUNE_MAX_ROWS};
