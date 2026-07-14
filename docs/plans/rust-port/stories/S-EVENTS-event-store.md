# Story S-EVENTS: SQLite Event Store

> **Phase:** 1 | **Depends on:** S-INTERFACES | **Go source:** `internal/events/` (395 lines)

## Summary

Port the append-only event log backed by SQLite (WAL mode). Implements
`EventStore` trait: `Append`, `Query`, `QueryAll`, plus retention pruning.

## Go Source

`internal/events/events.go` — `Store` struct, `New(dbPath)`, WAL mode,
`busy_timeout=5000`, `MaxOpenConns(1)` to serialize writes, `Append`,
`Query`, `QueryAll`, retention pruning.

## Rust Implementation

- Use `rusqlite` (bundled feature) — see
  `docs/rust-ecosystem/data-and-concurrency.md`
- Single `Connection` wrapped in `std::sync::Mutex`, DB ops via
  `tokio::task::spawn_blocking` (SQLite calls are blocking)
- PRAGMAs: `journal_mode=WAL`, `busy_timeout=5000`
- Schema: same as Go (`events` table with id, type, session_id, timestamp,
  payload JSON)
- Implements `EventStore` trait from S-INTERFACES
- Port `events_test.go`

## Acceptance Criteria

- [ ] `Append` writes event, returns with assigned ID
- [ ] `Query(sessionID, afterID, limit)` returns correct events
- [ ] `QueryAll(afterID, limit)` returns across all sessions
- [ ] WAL mode enabled, busy_timeout set
- [ ] Retention pruning works
- [ ] `cargo test events` passes
- [ ] No SQLite lock contention under concurrent appends
