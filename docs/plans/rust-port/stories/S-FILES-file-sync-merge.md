# Story S-FILES: File Sync & Three-Way Merge

> **Phase:** 2 | **Depends on:** S-PATHUTIL | **Go source:** `internal/files/` (254 lines)

## Summary

Port revision tracking (monotonic per-file revision numbers), three-way
merge (base/current/incoming), per-file locking, LRU base-content cache.

## Go Source

`internal/files/files.go` — `FileSync` struct, per-file `sync.Mutex` map,
`revisions` map, `lruCache` (256 entries, hand-rolled via `container/list`),
`WriteFile` with optimistic revision check, three-way merge on stale
revision.

## Rust Implementation

- Per-file locks: `DashMap<String, Arc<tokio::sync::Mutex<()>>>` or
  `HashMap<String, Arc<Mutex<()>>>` under a short-lived map lock
- LRU cache: `lru` crate (replaces hand-rolled `container/list` LRU)
- Three-way merge: port the diff3-style algorithm directly — it's pure
  string logic. Consider `diffs` or `similar` crate for diff primitives if
  the hand-rolled algorithm is complex
- 48-bit content hash: `sha2::Sha256` truncated, or `blake3`
- Port `files_test.go` (merge edge cases are critical)

## Acceptance Criteria

- [ ] Optimistic revision check works (stale → merge attempt)
- [ ] Three-way merge produces correct results for all test cases
- [ ] LRU cache bounded at 256 entries
- [ ] Per-file locks don't block concurrent writes to different files
- [ ] `cargo test files` passes
