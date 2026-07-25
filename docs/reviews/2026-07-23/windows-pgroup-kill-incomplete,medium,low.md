# Windows: process-group kill only terminates the immediate child

- **Difficulty:** medium
- **Urgency:** low
- **File:** `src/procutil/mod.rs`
- **Lines:** 55-70 (stub), 114-122 (`CREATE_NEW_PROCESS_GROUP`)

## Description

On Windows, `ProcessGroupCleanup` is a no-op (procutil/mod.rs:57-70) and `configure_process_group_inner` only sets `CREATE_NEW_PROCESS_GROUP` (procutil/mod.rs:114-122). Cancellation falls back to `child.kill().await` (shell/mod.rs:330-335), which terminates only the direct child. A shell pipeline (`cmd /C "a | b"`) or a backgrounded process leaves grandchildren running with workspace access after the session closes. The module doc (procutil/mod.rs:9-11) acknowledges this and defers to a Job Object.

## Recommendation

Assign the child to a Win32 Job Object with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` so that closing the job handle (on session cancel / daemon exit) kills the entire tree. This is the standard Windows equivalent of the Unix process-group kill.

## Verification

procutil/mod.rs:57-70 — `ProcessGroupCleanup::new`/`disarm` are no-ops on non-unix. procutil/mod.rs:114-122 — only `CREATE_NEW_PROCESS_GROUP` is set, no Job Object. shell/mod.rs:330-335 — the `#[cfg(windows)]` cancel path calls only `child.kill().await`.
