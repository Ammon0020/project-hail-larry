# App-write suppression race — NoteAppWrite recorded after the write it's meant to suppress

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `internal/fswatch/watcher.go`
- **Lines:** 131-137 (contract), 208-212 (check)

## Description

`NoteAppWrite` is documented as recording "that the app itself just wrote absPath so the imminent fsnotify event for it is suppressed." But the sole production caller (`internal/workspace/workspace.go` lines 417-429) calls `os.WriteFile` first and only then invokes the `onWrite` hook. The fsnotify event is delivered to the watcher's `loop` goroutine asynchronously through a channel, so there is no ordering guarantee that `NoteAppWrite` populates `w.appWrites` before `handle` checks it. When the event wins the race, the app's own write is incorrectly broadcast as `EventFileChangedOnDisk`. The test (`TestAppWriteIsSuppressed`, watcher_test.go:60-77) masks this by calling `w.NoteAppWrite(p)` *before* `os.WriteFile`, which is the opposite of the production ordering. Impact is benign (a spurious editor refresh of a tab whose content is already current), but the suppression guarantee is unreliable.

## Recommendation

Either (a) in `workspace.go`, call the `onWrite` hook *before* `os.WriteFile` so the timestamp is recorded pre-write, or (b) change the watcher's contract to accept a pre-write signal and document that ordering. The test should be updated to call `NoteAppWrite` after the write (matching production) and use a synchronization barrier to demonstrate the race, or better, the fix should make post-write registration also suppress by recording the timestamp just before the write.

## Verification

Read `internal/workspace/workspace.go` lines 417-429: `os.WriteFile` at 417, hook read at 425-427, hook called at 428-429. Read `watcher.go` lines 208-212: suppression check compares `time.Since(t)` against `appWriteSuppression`. Read `watcher_test.go` lines 68-72: `w.NoteAppWrite(p)` precedes `os.WriteFile(p, ...)`. The production and test orderings differ.
