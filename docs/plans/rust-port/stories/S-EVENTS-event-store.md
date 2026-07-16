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
- A dedicated blocking database boundary owns the single `rusqlite`
  connection; callers never hold a DB lock across `.await`.
- PRAGMAs: `journal_mode=WAL`, `busy_timeout=5000`.
- Schema, column types, indexes, IDs, and payload encoding exactly match the
  Go-created database fixture owned by S-MIGRATE.
- Append assigns the durable monotonic ID before `EventPublisher` makes the
  event visible to subscribers. The sync handoff is subscribe → replay →
  deduplicate by ID → live delivery.
- Implements the narrow event-store/publisher contracts from S-INTERFACES.
- Port `events_test.go` and add concurrent append/replay ordering tests.

## Acceptance Criteria

- [x] `Append` writes event, returns with assigned ID
- [x] `Query(sessionID, afterID, limit)` returns correct events
- [x] `QueryAll(afterID, limit)` returns across all sessions
- [x] WAL mode enabled, busy_timeout set
- [x] Retention pruning works
- [x] `cargo test events` passes
- [x] No SQLite lock contention under concurrent appends
- [x] Persist-before-publish ordering and reconnect replay handoff are tested
- [ ] Opens the Go-created event DB fixture without schema or payload drift
  (S-MIGRATE owns the fixture; schema/payload here match Go)
