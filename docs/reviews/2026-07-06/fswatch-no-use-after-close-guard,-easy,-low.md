# No use-after-close guard on public methods

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/fswatch/watcher.go`
- **Lines:** 88-137, 140-145

## Description

After `Close()`, the `loop` goroutine exits and `w.fsw` is closed, but `AddWorkspace`/`addTree`/`RemoveWorkspace` still call `w.fsw.Add`/`Remove`/`WatchList` (returning `fsnotify.ErrClosed`, logged at line 108) and `NoteAppWrite` still inserts into `w.appWrites` (which is never cleaned since `loop` is gone). These are wired to runtime HTTP handlers (`SetOnRegister`/`SetOnRemove`/`SetOnWrite` in daemon.go:274-276) that can fire during shutdown. Not a data race (fsnotify is internally locked), but the API silently does work on a dead watcher and leaks map entries.

## Recommendation

Have public methods check `<-w.done` (or a `closed` flag under `w.mu`) and return early when the watcher is shutting down.

## Verification

Read `watcher.go` 88-137 — no `select`/`done` check in any public method. Read daemon.go:273-276 — hooks are set on the workspace manager and can be invoked by HTTP handlers concurrently with `cleanup()` calling `fsWatcher.Close()` (daemon.go:373-375).
