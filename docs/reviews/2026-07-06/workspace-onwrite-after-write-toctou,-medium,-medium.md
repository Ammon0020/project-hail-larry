# App-write suppression registered after os.WriteFile, not before (TOCTOU race)

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `internal/workspace/workspace.go`
- **Lines:** 417-430

## Description

`WriteFile` calls `os.WriteFile` (line 417) and only afterward notifies the watcher via `onWrite(fullPath)` (lines 425-430). fsnotify delivers the write event to the watcher's `loop` goroutine asynchronously, and that goroutine can process the event (calling `handle` → checking `w.appWrites[path]`) before `NoteAppWrite` records the path in `fswatch.appWrites`. In that window the suppression entry does not exist yet, so the app's own write is emitted as `EventFileChangedOnDisk` — exactly the false positive the suppression is meant to prevent. The 2-second `appWriteSuppression` window in fswatch only suppresses events processed after `NoteAppWrite` runs; it cannot help when the event wins the race. The window is small but widens under lock contention (the path from `WriteFile` to `NoteAppWrite` acquires `m.mu.RLock` then `w.mu.Lock`).

Note: This is the same root issue as the fswatch app-write suppression race finding, reported from the workspace caller side. Both should be addressed together.

## Recommendation

Register the suppression BEFORE the write: call `onWrite(fullPath)` (or a dedicated pre-write hook) before `os.WriteFile`, so the entry is in `appWrites` before any fsnotify event can be processed. The 2-second window already tolerates events arriving slightly later.

## Verification

Read `WriteFile` lines 417-430 (write, then notify) and `fswatch.handle` lines 208-212 (suppression lookup). `NoteAppWrite` (fswatch lines 133-137) only inserts into `appWrites` after the OS write has already produced an fsnotify event. There is no pre-write registration.
