# Story S-FILES: File Sync & Three-Way Merge

> **Phase:** 2 | **Depends on:** S-PATHUTIL, S-INTERFACES | **Go source:** `internal/files/` (254 lines)

## Summary

Port revision tracking (monotonic per-file revision numbers), three-way
merge (base/current/incoming), per-file locking, LRU base-content cache.

## Go Source

`internal/files/files.go` — `FileSync` struct, per-file `sync.Mutex` map,
`revisions` map, `lruCache` (256 entries, hand-rolled via `container/list`),
`WriteFile` with optimistic revision check, three-way merge on stale
revision.

## Rust Implementation

- Per-file locks: a bounded/cleaned `DashMap` (stable `dashmap` 6.x, not an
  RC) of `String → Arc<tokio::sync::Mutex<()>>`; lock entries must not grow
  indefinitely with arbitrary file paths (evict or GC unused keys).
- LRU cache: **`lru` crate** — do not re-port Go's hand-rolled `container/list`
  LRU (`maxContentsEntries = 256`).
- Merge / diff: Go Phase 1 returns `ErrStaleRevision` without server-side
  merge (UI uses `@codemirror/merge`). Match that first. When/if server-side
  three-way merge is implemented, use **`similar`** (or `diffy` /
  `diffy-imara` if conflict-marker output is required); keep reconciliation
  thin and test against contract fixtures.
- Durable writes of file content (if used for state bases) go through
  `crate::fsutil::atomic_write` when atomicity is required; workspace user-file
  writes stay on the normal path with pathutil containment.
- Preserve the existing 48-bit hash algorithm and output exactly until a
  versioned state migration deliberately changes it.
- Port `files_test.go` and add property tests for non-overlapping edits,
  conflicting edits, and revision/cache bounds.

## Acceptance Criteria

- [ ] Optimistic revision check works (stale → merge attempt)
- [ ] Three-way merge produces correct results for all test cases
- [ ] LRU cache bounded at 256 entries
- [ ] Per-file locks don't block concurrent writes to different files
- [ ] `cargo test files` passes
