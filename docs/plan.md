# Phase 1 Implementation Plan

Plan for bootstrapping and completing Phase 1 of the Local Agent Interface.
Built from `docs/plans/Blueprint.md` and `docs/development/TechStack.md`.

## Status Legend

- `[ ]` — Not started
- `[~]` — In progress
- `[x]` — Complete

---

## Phase 1 — Core Infrastructure

### 1. `scaffold` — Project groundwork

- **Scope:** Go module, directory structure, `go:embed` setup, Vite frontend scaffold, basic HTTP server, shared interfaces.
- **Files:** `go.mod`, `cmd/`, `internal/`, `web/`, `main.go` (or `cmd/app/main.go`), `internal/interfaces/*.go`
- **Dependencies:** None
- **Blueprint refs:** Sec 3 (Architecture), Sec 25 (Phase 1)
- **Acceptance criteria:**
  - `go build ./...` passes
  - `go test ./...` passes
  - `go vet ./...` passes
  - `npm run build` passes (inside `web/`)
  - `app start` serves the web UI on `0.0.0.0:7337`
  - Health check endpoint responds at `GET /health`
  - Shared interfaces are defined for event store, workspace manager, ACP client, and permission manager

### 2. `cli-daemon` — CLI and daemon lifecycle

- **Scope:** `cmd/app` CLI commands using cobra; daemon start/stop/status; config storage in `~/.local-agent/`
- **Files:** `cmd/app/main.go`, `cmd/app/*_commands.go`, `internal/daemon/*.go`, `internal/config/*.go`
- **Dependencies:** `scaffold`
- **Blueprint refs:** Sec 4 (Host Daemon), Sec 20 (Configuration)
- **Acceptance criteria:**
  - `app start` launches the daemon on `0.0.0.0:7337`
  - `app stop` stops the daemon
  - `app status` shows URL, LAN IP, port, workspaces, and paired devices
  - `app add-folder [path]` registers a workspace
  - `app pair` generates a QR code and mnemonic
  - `app devices` lists paired devices
  - `app revoke <id>` revokes a device
  - `app logs` tails daemon logs
  - Config stored in `~/.local-agent/`
  - `go test ./internal/daemon/...` and `go test ./internal/config/...` pass

### 3. `events` — Event system and SQLite persistence

- **Scope:** Event type definitions, SQLite schema, event append/query, state derivation
- **Files:** `internal/events/*.go`, `internal/events/store/*.go`
- **Dependencies:** `scaffold`
- **Blueprint refs:** Sec 11 (Event System)
- **Acceptance criteria:**
  - All event types from Blueprint Sec 11 defined as typed structs
  - SQLite schema stores events with session ID, timestamp, and payload
  - Append-only event store supports query by session ID and time range
  - `go test ./internal/events/...` passes

### 4. `pairing` — Device pairing and authentication

- **Scope:** Pairing sessions, QR code generation, mnemonic passcode, device credentials, lock screen API
- **Files:** `internal/pairing/*.go`
- **Dependencies:** `cli-daemon`
- **Blueprint refs:** Sec 19 (Authentication)
- **Acceptance criteria:**
  - `app pair` creates a single-use pairing session with a QR code and four-word mnemonic
  - `/api/pair/qr` endpoint returns pairing data
  - `/api/pair/verify` endpoint accepts passcode and issues a device credential
  - Revocation works via `app revoke` and settings UI
  - `go test ./internal/pairing/...` passes

### 5. `workspace` — Workspace management

- **Scope:** Workspace registration, file tree, git info, workspace config
- **Files:** `internal/workspace/*.go`
- **Dependencies:** `cli-daemon`
- **Blueprint refs:** Sec 13 (Workspace Management), Sec 14 (File System Access)
- **Acceptance criteria:**
  - `app add-folder .` registers a directory as a workspace
  - Workspace list persisted in config
  - File tree API returns a JSON tree of the workspace
  - Git branch/status exposed via API
  - `go test ./internal/workspace/...` passes

### 6. `acp-client` — ACP client layer

- **Scope:** ACP transport (stdio JSON-RPC), session lifecycle, prompt exchange, streaming, capability negotiation
- **Files:** `internal/acp/*.go`
- **Dependencies:** `workspace`, `events`
- **Blueprint refs:** Sec 6 (ACP Client Layer), Sec 7 (ACP Integration), Sec 9 (Agent Lifecycle), Sec 10 (Session Lifecycle)
- **Acceptance criteria:**
  - Spawns an agent process and communicates over stdin/stdout JSON-RPC
  - Session create/load/list/close implemented
  - Prompt exchange and streaming events work
  - Capability negotiation at init
  - `go test ./internal/acp/...` passes

### 7. `permissions` — Permission manager

- **Scope:** `session/request_permission` handling, prompt routing to all devices, allow/deny policies, audit log
- **Files:** `internal/permissions/*.go`
- **Dependencies:** `events`, `acp-client`
- **Blueprint refs:** Sec 8 (Permission Manager)
- **Acceptance criteria:**
  - Permission requests broadcast to all paired devices
  - First response wins
  - Decisions: allow-once, allow-session, allow-always, deny
  - Audit log persists decisions
  - `go test ./internal/permissions/...` passes

### 8. `ws-sync` — WebSocket multi-client synchronization

- **Scope:** WebSocket server, event broadcast, reconnection with missing-event sync, in-flight permission prompt re-presentation
- **Files:** `internal/sync/*.go`
- **Dependencies:** `events`
- **Blueprint refs:** Sec 12 (Multi-Client Synchronization)
- **Acceptance criteria:**
  - `/ws` endpoint accepts authenticated connections
  - Events broadcast to all paired clients
  - Reconnection syncs missing events
  - In-flight permission prompts re-presented on reconnect
  - `go test ./internal/sync/...` passes

### 9. `file-sync` — File sync and merge

- **Scope:** Revision tracking, `FileRevisionUpdated` events, three-way merge on save, live agent change indicator
- **Files:** `internal/files/*.go`
- **Dependencies:** `workspace`, `events`
- **Blueprint refs:** Sec 14 (File System Access — Client File Sync)
- **Acceptance criteria:**
  - File revisions increment on every write
  - `FileRevisionUpdated` events broadcast on change
  - Save with `expectedRevision` detects stale revisions
  - Three-way merge attempted on stale saves
  - `go test ./internal/files/...` passes

### 10. `shell-exec` — Shell execution

- **Scope:** Workspace-scoped subprocess runner, output streaming as events
- **Files:** `internal/shell/*.go`
- **Dependencies:** `acp-client`, `permissions`
- **Blueprint refs:** Sec 15 (Shell Execution)
- **Acceptance criteria:**
  - Shell commands run only within workspace boundaries
  - Stdout/stderr streamed as `ShellOutputStreamed` events
  - Exit code returned to agent via ACP
  - `go test ./internal/shell/...` passes

### 11. `frontend-shell` — React app shell

- **Scope:** React app shell, layout (sidebar, main area, mobile nav), chat/event stream view, command input, session list
- **Files:** `web/src/components/*.tsx`, `web/src/App.tsx`
- **Dependencies:** `scaffold`, `ws-sync`
- **Blueprint refs:** Sec 17 (UI Architecture), `mockup12.html`
- **Acceptance criteria:**
  - VS Code-style layout renders on desktop
  - Mobile bottom-nav layout works
  - Chat view renders events from WebSocket
  - Session list and agent/model selectors work
  - `npm run build` passes

### 12. `frontend-editor` — Editor pane

- **Scope:** CodeMirror 6 editor pane, file tree, diff view, merge UI, file save with `expectedRevision`
- **Files:** `web/src/components/editor/*.tsx`, `web/src/components/FileTree.tsx`
- **Dependencies:** `file-sync`, `frontend-shell`
- **Blueprint refs:** Sec 14, Sec 17 (Editor and File Viewing), `mockup12.html`
- **Acceptance criteria:**
  - CodeMirror 6 loads file content
  - Diff/merge view available
  - Save sends `expectedRevision`
  - Live agent change indicators show
  - `npm run build` passes

### 13. `frontend-pairing` — Pairing and permissions UI

- **Scope:** Lock screen, pairing flow (QR scan / mnemonic entry), permission dialog UI, settings panel
- **Files:** `web/src/components/LockScreen.tsx`, `web/src/components/MobileSettings.tsx`, `web/src/components/PermissionDialog.tsx`
- **Dependencies:** `pairing`, `permissions`, `frontend-shell`
- **Blueprint refs:** Sec 8, Sec 19, `mockup12.html`
- **Acceptance criteria:**
  - Lock screen blocks unpaired access
  - Passcode entry works
  - QR code display works
  - Permission dialogs render with allow/deny/session options
  - Settings panel shows devices and connection status
  - `npm run build` passes

---

## Progress

- [x] `scaffold`
- [x] `cli-daemon`
- [x] `events`
- [x] `pairing`
- [x] `workspace`
- [x] `acp-client`
- [x] `permissions`
- [x] `ws-sync`
- [x] `file-sync`
- [x] `shell-exec`
- [x] `frontend-shell`
- [x] `frontend-editor`
- [x] `frontend-pairing`
