# Project Status — Local Agent Interface

> Last updated: 2026-06-23. Source of truth for task-level status.
> See `docs/plan.md` for full task definitions and `docs/pass-off.md` for prior session context.
> ACP stability + chat UX work: see `docs/specs/ui-spec.md`, `docs/specs/backend-spec.md`, `docs/plans/acp-stability.md`.

## ACP Stability & Chat UX Pass (2026-06-23)

Backend (all green: `go test ./...`, `go vet`, `npm run build`, `npm run lint`, `.\build.ps1`):
- ✅ Client capabilities advertised (fs + terminal); terminal methods implemented (`internal/acp/terminal.go`).
- ✅ Permission prompts now reach the UI (`PermissionManager.SetCallback` → `PermissionRequested` event); UI responds with correct `requestId` + dynamic options.
- ✅ Conversations persist across restarts (`~/.local-agent/conversations.json`); rename/delete/rebind via `PATCH/DELETE /api/sessions/{id}`.
- ✅ Mid-conversation model/agent switch (client-side rebind; ACP has no model API in v0.13.5).
- ✅ Enriched SessionUpdate: thoughts, plans, tool kind/target/diff; agent stderr captured into failure events.
- ⚠️ Deferred: graceful `session/close` on shutdown (kill used); `session/load`/`resume`; mock-agent terminal/permission regression coverage (WI-15).

Frontend: connection indicator + reconnect re-sync, Stop/cancel, render of thoughts/plans/shell/file/system events, conversation rename/delete UI, active conversation persisted to localStorage.

## Verification Summary

| Check | Status |
|-------|--------|
| `go build ./...` | ✅ Pass |
| `go test ./...` | ✅ All packages pass |
| `go vet ./...` | ✅ Pass |
| `npm run build` | ✅ Pass (web/) |
| `npm run lint` (web) | ✅ Pass (no errors) |
| `..\build.ps1` | ✅ Pass (frontend embedded) |
| Runtime: `app start` serves UI | ✅ Clean startup, no autodetect noise |
| Runtime: pairing flow works | ⏳ Not verified this session |

## Phase 1 — Core Infrastructure: ~100% COMPLETE

14 tasks in `docs/plan.md` all marked `[x]`, but 3 are overstated. See gaps below.

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1 | scaffold | ✅ Done | Go module, dirs, go:embed, HTTP server, shared interfaces |
| 2 | cli-daemon | ✅ Done | Cobra CLI: start/stop/status/add-folder/pair/devices/revoke/logs |
| 3 | events | ✅ Done | SQLite event store, WAL mode, append/query, all event types |
| 4 | pairing | ✅ Done | QR + mnemonic passcode, device credentials, revocation |
| 5 | workspace | ⚠️ Partial | File tree + file read + file write work. Workspaces now load from config on daemon startup. No `remove-folder` or `list-folders` CLI commands. |
| 6 | acp-client | ✅ Done | Session lifecycle uses `coder/acp-go-sdk` for Agent Client Protocol communication. `transport.go` bridges ACP events to system managers. |
| 7 | permissions | ✅ Done | Request/response, allow-once/session/always, deny, audit |
| 8 | ws-sync | ✅ Done | WebSocket hub, broadcast, reconnection sync |
| 9 | file-sync | ✅ Done | Revision tracking, FileRevisionUpdated events, three-way merge |
| 10 | shell-exec | ✅ Done | Workspace-scoped subprocess, output streaming as events |
| 11 | frontend-shell | ✅ Done | React app shell, desktop/mobile layouts, chat, session list. Visual layout works; cross-panel data flow depends on Tasks 12 and 14. |
| 12 | frontend-editor | ✅ Done | CodeMirror 6 renders, tabs dynamically driven by open files, save button calls `api.saveFile` with optimistic locking. Diff view deferred. |
| 13 | frontend-pairing | ✅ Done | Lock screen, passcode entry, QR, permission dialogs, settings |
| 14 | integration | ✅ Done | Backend wiring complete. Frontend uses `useBackend` to read/save files and manage chat sessions. Mock data removed. Only the ACP transport itself remains a stub. |

## Architecture Overview

```
cmd/app/main.go          → Cobra CLI (start/stop/status/pair/...)
internal/daemon/         → Lifecycle, wires all managers into server
internal/server/         → HTTP server, go:embed frontend, REST API, /ws
internal/events/         → SQLite event store (WAL, append-only)
internal/pairing/        → QR + mnemonic pairing, device credentials
internal/workspace/      → Registration, file tree, git info
internal/acp/            → ACP (Agent Client Protocol) client using coder/acp-go-sdk, session lifecycle
internal/permissions/    → Permission request/response, policies
internal/sync/           → WebSocket hub, broadcast, reconnection
internal/files/          → Revision tracking, three-way merge
internal/shell/          → Workspace-scoped subprocess runner
internal/interfaces/     → Shared Go interfaces (EventStore, etc.)
web/                     → React 19 + Vite 8 + Tailwind v4 + shadcn/ui
  src/hooks/useBackend.ts → Real backend hook (REST + WebSocket)
  src/lib/api.ts          → REST API client
  src/components/         → 11+ UI components
```

## Recently Resolved Gaps (Phase 1)

### ACP Transport — ✅ RESOLVED
`internal/acp/transport.go` implements the `coder/acp-go-sdk` `Client` interface:
- Spawns agent process using `os/exec` inside the workspace directory
- Full Agent Client Protocol via `NewClientSideConnection` (stdio)
- Bridges `SessionUpdate` to system events (`StreamUpdate`, `ToolStarted`, `ToolCompleted`)
- Bridges `RequestPermission` to `PermissionManager` (prompts UI for allow/deny)
- `NewSessionRequest` now sends `mcpServers: []` instead of `null` to satisfy strict ACP agents (devstral-small). Regression test added.

### Agent Configuration UI & Autodetection — ✅ RESOLVED
- Implemented full Agent CRUD operations (`POST /api/agents`, `DELETE`, etc.) backed by local storage.
- Added `SettingsModal.tsx` and `MobileSettings.tsx` to configure agents from the UI.
- Implemented **dynamic model autodetection** in `internal/acp/autodetect.go` which modularly attempts an ACP `providers/list` handshake, and gracefully falls back to reading `models_cache.json` and `config.toml` for `codex` and `vibe`.

### Editor + File Explorer — ✅ RESOLVED
- **File tree:** Fully interactive. Folders expand/collapse, clicking files opens them in editor tabs.
- **Editor content:** Dynamically loads from backend (`api.readFile`), tracking language and unsaved state.
- **Editor tabs:** Fully manageable (select, close, track unsaved status).
- **Save button:** Wired to `api.saveFile` using optimistic locking (`expectedRevision`).

### Workspace Persistence — ✅ RESOLVED
- `workspace.Manager` workspaces now loaded from `~/.local-agent/config.json` on daemon start
- `WriteFile` endpoint added: `POST /api/workspaces/{id}/file` with optimistic locking
- `ListSessions` endpoint added: `GET /api/sessions`
- `app add-folder` now checks for duplicate registrations
- LeftSidebar workspace switcher wired to `backend.workspaces` with dropdown UI
- Missing CLI commands: no `app remove-folder`, no `app list-folders`
- **Note:** Must rebuild binary (`.\build.ps1`) after frontend changes — `go:embed` freezes frontend at compile time

## What's Left

### Runtime Verification Needed
- [ ] `app start` → browser connects → web UI loads
- [ ] `app add-folder .` → workspace appears in UI file tree
- [ ] `app pair` → QR/passcode → device pairs → lock screen clears
- [ ] Editor pane loads file content → save works
- [ ] **ACP transport** → spawn agent, send prompt, stream response (now implemented, needs end-to-end verification in UI)

### Open Items (from `docs/plans/OpenItems.md`)
- [ ] **TLS on LAN** — plain HTTP; needed before real network use
- [ ] **Pairing TTL** — currently 5 min hardcoded; no config
- [ ] **Device credential expiry** — permanent until revoked
- [ ] **Editor on mobile** — CodeMirror needs touch optimization
- [ ] **ACP sub-workers** — deferred until next ACP release

### UI & Chat Implementation Gaps
- [ ] **UI Persistence** — On reload, the UI loses state. Needs to maintain selected files, active model, and active conversation.
- [x] **Chat Messages** — Send prompt now persists/broadcasts events, awaits completion, shows errors, and renders all event types (PromptSubmitted, ResponseStarted, StreamUpdate, ToolStarted, ToolCompleted, AgentExited). Lint clean.
- [ ] **Conversation Management** — Missing the ability to rename existing conversations.

## Development Phases (from Blueprint Sec 25)

### Phase 1 — Core Infrastructure (current)
- Daemon + CLI, pairing, web server, workspace mgmt, session lifecycle
- ACP client layer, permission manager, shell execution, single agent
- Event system, WebSocket sync, CodeMirror 6 editor with diff view
- **Status: 100% done. Ready for Phase 2.**

### Phase 2 — Multi-Agent Support (not started)
- Agent registry (`internal/acp/` + new config persistence)
- Capability negotiation, multiple simultaneous workers
- Agent configuration and authentication, session resume support
- Permission policies and audit log (`internal/permissions/`), enhanced diagnostics

### Phase 3 — Advanced Features (not started)
- MCP management, multi-client collaboration (`internal/sync/`)
- Plugin architecture, session replay (`internal/events/`)
- Advanced workspace tools (`internal/workspace/`, `internal/files/`)
- Developer terminal (`internal/shell/`), UI polish and accessibility (`web/`)

## Key Decisions

- Go module: `github.com/adama/local-agent`
- Frontend dir: `web/` (not `frontend/`)
- Dark theme default; Tailwind v4 `@theme inline` pattern
- Event-driven UI: all rendering derives from `AppEvent[]` stream
- `useBackend` hook replaced `useMockBackend` — real WebSocket + REST
- Default agent registered: `claude-code` with Sonnet 4 / Opus 4 models
- SQLite via `modernc.org/sqlite` (pure-Go, no CGO)
