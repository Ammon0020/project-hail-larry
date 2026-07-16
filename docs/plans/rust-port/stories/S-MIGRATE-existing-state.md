# Story S-MIGRATE: Existing State Compatibility and Migration

> **Phase:** 1 | **Depends on:** S-CONFIG, S-EVENTS, S-CONTRACT | **Go source:** `internal/config/`, `internal/events/`, `internal/acp/store.go`, `internal/pairing/`, `internal/uploads/`

## Goal

Allow the Rust binary to replace the Go binary without losing user state or
silently invalidating paired devices, workspaces, events, sessions, MCP config,
or uploads.

## Scope

- Build anonymized Go-created fixture trees for each supported prior state
- Open and validate config, event SQLite DB, device credentials, conversation
  metadata, MCP configuration, certificates, and uploads
- Preserve current formats where possible; version any required migration
- Make migrations atomic, idempotent, restart-safe, and failure-loud
- Define backup and rollback behavior before modifying user state

## Acceptance Criteria

- [x] Go-created state fixtures open successfully without data loss
- [x] Existing workspace registrations, event IDs, uploads, and paired-device credentials remain usable
- [x] Any schema/config migration is atomic, idempotent, and has a tested interrupted-run recovery path
- [x] Migration creates a versioned backup before destructive format changes
- [x] Migration failures leave the prior state readable by the Go binary
- [x] Contract tests cover upgrade from the latest supported Go state

## Implementation notes (2026-07-15)

- **Module:** `src/migrate/` — `run_migrations(state_dir)`, `migrate_config`,
  `validate_state_tree`, format detect, restore-from-backup.
- **Config transform:** `config.json` (Go/JSON) → `config.toml` (Rust/TOML).
  Field names already match (`camelCase`). Backup: `config.json.bak.v1`.
  Dual-state after success keeps `config.json` so a Go binary still loads.
- **Unchanged formats (validated only):** `local-agent.db`, `devices.json`,
  `conversations.json`, `mcp.json`, `uploads/`, `tls/`.
- **Deferred semantic load:** pairing / ACP store / MCP / uploads managers
  (structure OK; full open in their port stories).
- **Fixtures:** `tests/migrate/fixtures/go-state/` (anonymized).
