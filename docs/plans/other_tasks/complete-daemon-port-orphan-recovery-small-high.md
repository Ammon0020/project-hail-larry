# Task: Daemon start/stop recover from port-holding orphan (no PID file)

> **Status:** done | **Urgency:** high | **Difficulty:** small
> **Scope:** `src/app/` daemon lifecycle (CLI `start` / `stop`)

## Problem

`local_agent stop` keys only off `~/.local-agent/daemon.pid`. `local_agent start`
checks liveness only via that PID file before calling `listen::bind`. When a
daemon is running but its PID file is missing (orphaned process, different
binary, crashed cleanup, manual kill of the wrong process), the CLI is stuck:

- `stop` reports `daemon is not running` even though the port is held.
- `start` then fails at `bind` with `Address already in use (os error 98)` and
  cannot recover without the user manually `kill`-ing the orphan.

Observed 2026-07-18: a `target/release/local-agent` daemon held port 7337 with
no PID file; `./bin/local_agent stop` said "not running" and `start` could not
bind.

## Desired behavior

- `start`: before constructing the daemon, probe the configured HTTP port. If
  something is already listening, fail fast with a clear message that names the
  port and (when discoverable) the holding PID, and point the user at `stop` or
  a manual kill. Never reach `bind` with an orphan holding the port.
- `stop`: when no live PID file exists, look for a process listening on the
  configured HTTP port. If found, SIGTERM it (with a warning that it is an
  unmanaged daemon) and clean up. If not found, keep the current "not running"
  message.

## Approach

- New `src/app/port.rs`:
  - `is_port_listening(host, port) -> bool` — cross-platform sync TCP connect
    probe (250ms timeout). Connect to `127.0.0.1` when host is empty or a
    wildcard, otherwise the configured host. Returns false on any error.
  - `find_pid_listening_on(port) -> Result<Option<u32>>` — Linux: parse
    `/proc/net/tcp` + `/proc/net/tcp6` for LISTEN rows on the port, collect
    socket inodes, then scan `/proc/*/fd/*` for `socket:[inode]` matches.
    Non-Linux: return `Ok(None)` (deferred; logged).
- `cli/mod.rs::start`: after the PID-file `status` check, when `config.port !=
  0`, run the port probe (via `spawn_blocking`). If listening, bail with the PID
  when `find_pid_listening_on` returns one.
- `app/daemon.rs::stop`: when `read_live_pid` returns `None`, call
  `port::find_pid_listening_on(config.port)`. If `Some(pid)`, warn that it is an
  unmanaged daemon, `process::stop(pid)`, and remove any stale PID file.

## Acceptance criteria

- [x] `start` against an orphaned port-holder fails with a clear, actionable
      message instead of `Address already in use`.
- [x] `stop` with no PID file but a live port-holder terminates that process.
- [x] `start`/`stop` still work normally when the PID file is present and live.
- [x] Port probe is skipped for `port == 0` (OS-assigned, used in tests).
- [x] Unit tests cover: probe true for a bound listener, false for a free port,
      and (Linux) `find_pid_listening_on` returns the test process's PID.
- [x] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D
      warnings`, `cargo fmt --check -q` all pass.

## Out of scope

- macOS/Windows PID-from-port (logged as deferred in `docs/known-issues.md`).
- Migrating to a socket-activation or SO_REUSEPORT model.
- Detecting orphans on the HTTPS port (HTTP port is the canonical probe).
