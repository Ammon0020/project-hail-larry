# Story S-FSWATCH: On-Disk Change Detection

> **Phase:** 2 | **Depends on:** S-EVENTS | **Go source:** `internal/fswatch/` (353 lines)

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
- Use a maintained `notify` debouncer rather than a hand-rolled sleep loop.
- Subscribe to narrow app-write/workspace lifecycle notifications for
  suppression and root updates; do not add workspace setter callbacks.
- Suppress app writes with bounded TTL state and explicit cleanup.
  (recently-written set)
- Recursive watch: `notify::RecursiveMode::Recursive`
- Port `watcher_test.go`

## Acceptance Criteria

- [ ] External file changes detected and emitted
- [ ] App-originated writes suppressed
- [ ] Rapid changes debounced (no event storm)
- [ ] Watcher handles file creation, modification, deletion
- [ ] `cargo test fswatch` passes
