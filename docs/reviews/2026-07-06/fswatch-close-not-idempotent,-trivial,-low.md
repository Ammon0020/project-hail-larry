# Close() panics on second call (close of closed done channel)

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `internal/fswatch/watcher.go`
- **Lines:** 140-145

## Description

`Close()` calls `close(w.done)` unconditionally. A second `Close()` (e.g., a cleanup path that already ran, or a test + daemon both closing) panics with "close of closed channel". The daemon's `cleanup()` calls it once, but the API offers no guard.

## Recommendation

Guard with `sync.Once` or a closed flag under `w.mu`: `select { case <-w.done: return nil; default: close(w.done) }`.

## Verification

Read `watcher.go` lines 140-145; `close(w.done)` has no guard. No `sync.Once` field exists on the struct (lines 50-61).
