# Story S-SHELL: Workspace Subprocess Runner

> **Phase:** 2 | **Depends on:** S-PATHUTIL | **Go source:** `internal/shell/` (320 lines)

## Summary

Port workspace-scoped shell command execution: spawn subprocess, stream
stdout/stderr, enforce workspace CWD, capture exit codes, timeout handling.

## Go Source

`internal/shell/shell.go` — `Executor`, `Run(ctx, command, cwd)`, streams
output via channels, enforces workspace path bounds, handles signals.

## Rust Implementation

- `tokio::process::Command` (async subprocess)
- Stream stdout/stderr: `tokio::process::Child::stdout()` → read lines
  via `tokio::io::BufReader` → send to channel
- CWD enforcement: validate against workspace root via S-PATHUTIL
- Cancellation: a per-command `CancellationToken` terminates the spawned
  process group/tree, not merely the immediate child; provide equivalent
  Unix and Windows implementations behind `#[cfg]`.
- Exit code: `child.wait().await` → `ExitStatus::code()`.
- Bound captured/streamed output and test cancellation races so a noisy or
  orphaned command cannot exhaust memory or survive daemon shutdown.
- Port `shell_test.go`

## Acceptance Criteria

- [x] Commands run in workspace CWD
- [x] stdout/stderr streamed line-by-line
- [x] Exit codes captured correctly
- [x] Cancellation kills the process
- [x] Path traversal in CWD rejected
- [x] `cargo test shell` passes
