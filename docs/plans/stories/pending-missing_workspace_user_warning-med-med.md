# Story: Missing workspace — user-visible warning (not auto-prune)

> **Status:** done (Rust) / pending (Go prune revert) | **Urgency:** med | **Difficulty:** med  
> **Scope:** Go daemon (and Rust port parity when S-WORKSPACE / S-DAEMON land)

## Goal

When a registered workspace path is missing or invalid at daemon startup (or
when listing workspaces), **do not silently remove it from config**. Instead
**surface a clear warning to the user** so they can reconnect the drive, fix the
path, or remove the folder themselves.

## Background / current behavior

As of 2026-07-15, `internal/daemon/daemon.go` registration loop:

1. `workspaceMgr.Register` fails (e.g. path not found)
2. Logs `WARNING: failed to load workspace … — removing from config`
3. Calls `appCfg.RemoveWorkspacePath(wsPath)` and persists the drop

That stops log spam for stale fixture paths but is the wrong product behavior
for real user folders (external drive unplugged, renamed directory, temporary
network mount). Auto-prune is surprising and destructive without consent.

## Desired behavior

- Keep the path in `config.Workspaces` until the user removes it via
  `app remove-folder` or Settings.
- Show a **user-visible** warning, not only a daemon log line, e.g.:
  - Settings / workspace list: status chip or row banner (“Path missing” /
    “Unavailable — check the folder exists”)
  - Optional: toast or startup system message on first load after start
  - `app status` / `app list-folders`: include unavailable workspaces with a
    clear flag
- API: list workspaces should still return the entry (or a dedicated
  “unavailable” entry) so the UI can render the warning. Define wire fields
  (e.g. `available: false`, `error: "…"`) in S-CONTRACT if the shape changes.
- Daemon startup: log WARNING but **do not** call `RemoveWorkspacePath`.
- Rust port must match this policy once workspace loading is implemented
  (do not copy auto-prune).

## Acceptance criteria

- [x] Missing/invalid workspace paths stay registered until the user removes them
- [x] UI (and CLI status/list) shows a clear warning for unavailable workspaces
- [x] Daemon logs a warning but does not auto-delete from config
- [x] User can still `app remove-folder` / Settings remove to clean up
- [ ] Optional: “retry load” once path becomes available (watch or manual refresh)
- [x] Tests cover “path gone → still in config + unavailable in list”
- [ ] Revert or gate the temporary auto-prune in `daemon.go` when this ships

## Notes for implementor

- Temporary auto-prune was added to fix startup spam from a doubled
  contract-fixture path (`…/go-fixtures/tests/contract/fixtures/seed-workspace`).
  Prefer UX over silent mutation.
- Coordinate with frontend workspace list / settings tab; avoid only fixing the
  log stream.

## Out of scope

- Full offline-workspace sync
- Automatically recreating deleted directories
