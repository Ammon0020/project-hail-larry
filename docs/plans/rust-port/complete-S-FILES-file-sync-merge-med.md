# Story S-FILES: File Sync & Three-Way Merge

> **Phase:** 2 | **Depends on:** S-PATHUTIL, S-INTERFACES | **Go source:** `internal/files/`
> **Status:** complete (2026-07-18) — server Phase 1 parity; merge is frontend-owned.

## Summary

Port revision tracking (monotonic per-file revision numbers), optimistic
`StaleRevision` on conflict, per-file locking, LRU base-content cache.
Three-way merge remains frontend-owned (matches Go Phase 1).

## Go Source

`internal/files/files.go` — `FileSync` struct, per-file `sync.Mutex` map,
`revisions` map, `lruCache` (256 entries), `WriteFile` with optimistic
revision check. Phase 1 returns `ErrStaleRevision` without server-side merge.

## Rust Implementation

- Per-file locks: `DashMap` of `String → Arc<tokio::sync::Mutex<()>>` in
  `src/files/mod.rs` with lock-map GC.
- LRU cache: `lru` crate, `MAX_CONTENTS_ENTRIES = 256`.
- Stale path: returns `AppError::StaleRevision`; `get_base_content` feeds the
  UI merge. **No server-side three-way merge** — intentional Phase 1 parity.
- 48-bit content-hash revisions preserved.
- Tests in `src/files/tests.rs`.

## Architecture decision (merge)

Server returns `StaleRevision` + base content; frontend performs three-way
merge via `@codemirror/merge`. Server-side merge (`similar`/`diffy`) is out
of scope unless a later epic requires it. Formal deferral:
`docs/plans/other_tasks/complete-three-way-merge-deferral-or-impl-med-low.md`.

## Acceptance Criteria

- [x] Optimistic revision check works (stale → `StaleRevision`, not silent overwrite)
- [x] Three-way merge: **deferred to frontend** (`@codemirror/merge`); server
  Phase 1 returns `StaleRevision` + base content (Go parity) — not a port gap
- [x] LRU cache bounded at 256 entries
- [x] Per-file locks don't block concurrent writes to different files
- [x] `cargo test` files module passes
