//! SQLite event store (Go `internal/events/`).
//!
//! WAL-mode, append-only event log with retention pruning, query, and replay.
//! Uses `rusqlite` (bundled) behind a blocking boundary; callers must never
//! hold a lock across `.await`. Implementation lands in S-EVENTS (Phase 1).
//! S-ARCH scope: module placeholder only.
