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

- [ ] Go-created state fixtures open successfully without data loss
- [ ] Existing workspace registrations, event IDs, uploads, and paired-device credentials remain usable
- [ ] Any schema/config migration is atomic, idempotent, and has a tested interrupted-run recovery path
- [ ] Migration creates a versioned backup before destructive format changes
- [ ] Migration failures leave the prior state readable by the Go binary
- [ ] Contract tests cover upgrade from the latest supported Go state
