# Story S-CLI: CLI Commands (clap)

> **Phase:** 4 | **Depends on:** S-DAEMON | **Go source:** `cmd/app/` (~600 lines)

## Summary

Port the CLI: all cobra subcommands → clap subcommands. Commands: start,
stop, status, add-folder, remove-folder, list-folders, pair, devices,
revoke, install-service, uninstall-service, logs.

## Go Source

`cmd/app/main.go` + `service_linux.go`, `service_darwin.go`,
`service_windows.go`, `service_other.go` — cobra command tree, platform-
specific service install/uninstall.

## Rust Implementation

- `clap` derive (see `docs/rust-ecosystem/cli-and-config.md`)
- Platform service: `#[cfg(target_os)]` modules
  - Linux: systemd unit generation + install
  - macOS: launchd plist generation + install
  - Windows: preserve the existing HKCU registry entry behavior; do not add a
    Windows service manager during the parity port.
- `start --background`: use small platform modules with native process APIs;
  avoid a daemonization crate unless S-ARCH finds a tested cross-platform need.
- Port service tests

## Acceptance Criteria

- [ ] All CLI commands work identically to Go version
- [ ] `app start` / `app start --background` work
- [ ] `app pair` generates QR + mnemonic
- [ ] `app install-service` / `app uninstall-service` work per platform
- [ ] `app logs` shows recent log output
- [ ] `cargo test cli` passes
