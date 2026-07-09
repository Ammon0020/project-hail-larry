# Synchronous emit blocks the single event loop

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `internal/fswatch/watcher.go`
- **Lines:** 227-233 (call), 147-169 (loop)

## Description

`handle` invokes `emit(interfaces.Event{...})` inline on the single `loop` goroutine. The wired emit is `srv.OnEvent` (daemon.go:270), which calls `EventStore.Append` (a synchronous SQLite insert) and then `SyncHub.Broadcast` (server.go:354-372). Every filesystem event therefore serializes through a SQLite write on the watcher's only event-processing goroutine. Under a burst of changes (e.g., `git checkout` touching many files), the fsnotify `Events` channel can fill (fsnotify uses an unbuffered/short internal queue on some backends) and events get dropped, or the OS watch buffer overflows (Linux inotify) producing `ENOSPC`/`Q_OVERFLOW` errors. There is also no backpressure handling for a blocked broadcast.

## Recommendation

Push events onto a buffered channel and have a separate emitter goroutine drain it (decouples I/O from event intake), or document the contract that `emit` must be non-blocking. At minimum, bound the work per event.

## Verification

Read `watcher.go` 227-233 — `emit(...)` called directly in `handle`, which is called from the `case ev := <-w.fsw.Events` arm of the single `loop` goroutine (lines 155-159). Read `server.go` 354-372 — `OnEvent` → `recordEvent` → `EventStore.Append` (SQLite) + `Broadcast`, all synchronous.
