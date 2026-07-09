# Tests don't exercise production ordering, recursive Create, throttle, or Close safety

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/fswatch/watcher_test.go`
- **Lines:** 1-110

## Description

(a) `TestAppWriteIsSuppressed` (60-77) calls `NoteAppWrite` before the write, opposite of the real caller — see the app-write suppression race finding. (b) No test for the `Create`-directory → `addTree` recursive-watch path (watcher.go:181-188), which is the mechanism that keeps newly created subdirectories watched; a regression here silently breaks recursive watching. (c) No test for the `emitThrottle` coalescing (watcher.go:213-218). (d) No test that `Close()` is safe to call twice or that events stop after close. (e) No test for `Close()` concurrent with `AddWorkspace`. (f) All tests use `time.Sleep` for synchronization rather than deterministically driving the watcher, making them flaky on slow/loaded CI.

## Recommendation

Add a test that creates a nested directory after `AddWorkspace` and asserts a write inside it emits an event; add a double-`Close()` test; add a throttle test that writes the same file twice quickly and asserts exactly one event; flip the suppression test to call `NoteAppWrite` after the write to match production.

## Verification

Read the entire test file (110 lines): only four tests exist, covering happy path, suppression (with wrong ordering), ignored dir, and remove. No test references `addTree`, `emitThrottle`, double-close, or post-write suppression.
