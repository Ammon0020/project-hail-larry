# Project Status — Local Agent Interface

> Last updated: 2026-06-20. Source of truth for task-level status.
> See `docs/plan.md` for full task definitions and `docs/pass-off.md` for prior session context.

## Verification Summary

| Check | Status |
|-------|--------|
| `go build ./...` | ✅ Pass |
| `go test ./...` | ✅ All packages pass |
| `go vet ./...` | ✅ Pass |
| `npm run build` | ✅ Pass (web/) |
| Runtime: `app start` serves UI | ⏳ Not verified this session |
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
| 6 | acp-client | ⚠️ Stub | Session lifecycle in-memory; **no actual stdio JSON-RPC transport**. SendPrompt emits events but doesn't spawn/communicate with agent processes. See Critical Gaps below. |
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
internal/acp/            → ACP JSON-RPC stdio client, session lifecycle
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

## Critical Gaps in Phase 1

### ACP Transport — NOT IMPLEMENTED (highest priority)
`internal/acp/acp.go` — client is a stub:
- `CreateSession` creates an in-memory record but does **not** spawn an agent process via `os/exec`
- `SendPrompt` emits a `PromptSubmitted` event but does **not** send JSON-RPC over stdio
- The `Session.cmd *exec.Cmd` field is never populated
- No ACP protocol handshake, capability negotiation, or response streaming
- **Impact:** you cannot talk to any real agent (Claude Code, Mistral, etc.)
- **To test with Mistral vibe:** need to implement the actual stdio JSON-RPC transport in `internal/acp/`

### Agent Configuration UI — MISSING
- `internal/daemon/daemon.go:90-98` — hardcodes one agent (`claude-code`) at startup
- No UI to register/configure agents (command, models, auth)
- ChatPanel has agent/model selectors but they only list what the daemon returns
- Agent registry is in-memory only (no persistence)

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
- [ ] **ACP transport** → spawn agent, send prompt, stream response (blocked by stub)

### Open Items (from `docs/plans/OpenItems.md`)
- [ ] **TLS on LAN** — plain HTTP; needed before real network use
- [ ] **Pairing TTL** — currently 5 min hardcoded; no config
- [ ] **Device credential expiry** — permanent until revoked
- [ ] **Editor on mobile** — CodeMirror needs touch optimization
- [ ] **ACP sub-workers** — deferred until next ACP release

## Development Phases (from Blueprint Sec 25)

### Phase 1 — Core Infrastructure (current)
- Daemon + CLI, pairing, web server, workspace mgmt, session lifecycle
- ACP client layer, permission manager, shell execution, single agent
- Event system, WebSocket sync, CodeMirror 6 editor with diff view
- **Status: ~100% done. Only gap is ACP transport stub (bridge to Phase 2).**

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
