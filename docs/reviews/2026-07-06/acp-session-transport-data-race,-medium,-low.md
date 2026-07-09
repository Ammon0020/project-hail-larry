# Unsynchronized read of session.transport in prompt goroutine races with closeTransportLocked

- **Difficulty:** medium
- **Urgency:** low
- **File:** `internal/acp/acp.go`
- **Lines:** 429

## Description

The prompt goroutine launched in `SendPrompt` reads `session.transport` at line 429 (`session.transport.Prompt(...)`) and again at line 434 (`session.transport.StderrTail()`) without holding `c.mu`. Meanwhile, `closeTransportLocked` (line 583) writes `session.transport = nil` under `c.mu`, and is invoked by `CloseAllSessions` during daemon shutdown. If a shutdown coincides with an in-flight prompt, the goroutine can read `nil` and panic with a nil-pointer dereference, or the `-race` detector would flag an unsynchronized concurrent read/write. This race is **pre-existing** (the line existed before this diff), but the diff modified this exact line to add the `attachments` argument, so it is in scope. The `-race` test passes only because no test triggers `CloseAllSessions` concurrently with an in-flight prompt.

## Recommendation

Capture `tr := session.transport` while still holding `c.mu` (before launching the goroutine), then use `tr` inside the goroutine. Also nil-check `tr` before calling `Prompt`. This eliminates the race and the potential nil-deref.

## Verification

Read `acp.go:381` (`c.mu.Unlock()`) followed by `go func()` at 418 reading `session.transport` at 429 without re-acquiring the lock. Read `closeTransportLocked` at 563-599 which sets `session.transport = nil` at 583 under `c.mu`. Confirmed `go test -race -count=1 ./internal/acp/` passes — but only because no test exercises the concurrent shutdown path.
