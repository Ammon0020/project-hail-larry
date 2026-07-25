# No per-command execution timeout — terminal commands run until session close

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/shell/mod.rs`
- **Lines:** 218-326 (`run_inner`); `src/acp/core.rs:2364-2404`

## Description

`run_inner` races only `child.wait()` against `token.cancelled()`. There is no `tokio::time::timeout`. The terminal's cancel token is `deps.cancellation.child_token()` (core.rs:2365) — i.e. the **session** cancellation, not a per-command deadline. An agent can spawn `sleep 1000000` (up to 16 of them per session, `MAX_TERMINALS_PER_SESSION=16`) that pin a process slot and consume a PIPED stdout reader task each until the session is closed or a device manually calls `kill_terminal`. There is no automatic reaping of long-running commands and no resource accounting beyond the terminal count.

## Recommendation

Wrap `run_inner`'s wait in `tokio::time::timeout(MAX_EXECUTION_DURATION, child.wait())`, cancelling the token on expiry. Surface a configurable per-session default (e.g. 10 minutes) and let the prompt disclose the deadline. Consider a global cap on concurrent terminals across all sessions, not just per-session.

## Verification

shell/mod.rs:310-326 — the `tokio::select!` has only `token.cancelled()` and `child.wait()` branches; no timeout branch. core.rs:2365 — `cancel: deps.cancellation.child_token()` is session-scoped. No `tokio::time::timeout` appears anywhere in shell/mod.rs (grep confirmed).
