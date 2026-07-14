# Story S-DAEMON: Daemon Lifecycle & Wiring

> **Phase:** 4 | **Depends on:** S-SERVER and all service stories | **Go source:** `internal/daemon/` (773 lines)

## Summary

Port daemon lifecycle: start/stop/status, wire all managers together,
graceful shutdown, platform-specific process management (SIGTERM on Unix,
taskkill on Windows).

## Go Source

`internal/daemon/daemon.go` — `Daemon` struct, `Start`, `Stop`, `Status`,
wires all managers (config, events, pairing, workspace, acp, permissions,
sync, server, uploads). Platform files: `process_unix.go`, `process_windows.go`,
`stop_unix.go`, `stop_windows.go`.

## Rust Implementation

- `App`/`Daemon` composition root constructs every manager with explicit
  constructor dependencies; no setter-based temporal coupling.
- `Start`: initialize all managers in dependency order, configure structured
  file logging at `~/.local-agent/logs/`, install the chosen rustls crypto
  provider before TLS, then start the server.
- `Stop`: graceful shutdown — drain HTTP requests, close WS connections,
  cancel all tasks via `CancellationToken`, close SQLite
- Platform process management: `#[cfg(target_os)]` modules
  - Unix: send SIGTERM to PID, wait
  - Windows: `taskkill /PID` equivalent
- Port `daemon_test.go`

## Acceptance Criteria

- [ ] Daemon starts all managers in correct order with no post-construction wiring
- [ ] `app logs` has a stable rolling file-log source
- [ ] TLS starts without an implicit rustls crypto-provider choice
- [ ] Graceful shutdown drains in-flight requests
- [ ] Platform-specific stop works (Unix SIGTERM, Windows taskkill)
- [ ] Status reports running/stopped + bound addresses
- [ ] `cargo test daemon` passes
