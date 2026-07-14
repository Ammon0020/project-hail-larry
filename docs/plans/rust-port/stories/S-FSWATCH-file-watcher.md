# Story S-FSWATCH: On-Disk Change Detection

> **Phase:** 2 | **Depends on:** — | **Go source:** `internal/fswatch/` (353 lines)

## Summary

Port filesystem watcher that detects external file changes (files edited
outside the app) and emits `FileChangedOnDisk` events, suppressing
app-originated writes.

## Go Source

`internal/fswatch/watcher.go` — uses `fsnotify`, watches workspace dirs
recursively, debounces rapid changes, suppresses writes made by the app
itself (via a "recently written" set with TTL).

## Rust Implementation

- `notify` crate (replaces `fsnotify`) — see
  `docs/rust-ecosystem/data-and-concurrency.md`
- Debounce: `tokio::time::sleep` or a debounce channel
- Suppress app writes: `DashMap<PathBuf, Instant>` with TTL cleanup
  (recently-written set)
- Recursive watch: `notify::RecursiveMode::Recursive`
- Port `watcher_test.go`

## Acceptance Criteria

- [ ] External file changes detected and emitted
- [ ] App-originated writes suppressed
- [ ] Rapid changes debounced (no event storm)
- [ ] Watcher handles file creation, modification, deletion
- [ ] `cargo test fswatch` passes
