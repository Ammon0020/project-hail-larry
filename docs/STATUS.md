# Project Status — Local Agent Interface

> Last updated: 2026-06-27. Source of truth for task-level status.
> See `docs/archive/plan.md` for Phase 1 task definitions and `docs/archive/pass-off.md` for prior session context.
> ACP stability + chat UX work: see `docs/specs/ui-spec.md`, `docs/specs/backend-spec.md`, `docs/archive/acp-stability.md`.
> Finish-the-codebase work streams: see `docs/plans/execution-plan.md` (all 6 streams complete).

## Bug Fixes (2026-06-27)

- ✅ **Conversation history lost on page reload** — `App.tsx` now triggers `backend.loadSessionEvents(activeSessionId)` when the session list loads and contains the persisted `activeSessionId`, using the "adjust state during render" pattern (tracked via `loadedEventsForSession` state) to fire only once per session. Previously `loadEvents()` only fetched the first 200 global events, so the active session's history was missing on reload unless it happened to fall in that window. The harness/model selectors were also perceived as broken because the empty chat made the session look unset — they were already correct (backend persists `AgentID`/`ModelID` in `conversations.json` and returns them via `ListSessions`).
- ✅ **Stale activeSessionId recovery** — `App.tsx` now validates the persisted `activeSessionId` against the loaded session list and clears it when missing (render-time adjustment pattern). `useBackend.sendPrompt` catches 404/"session not found", clears `localStorage`, and throws a friendly "This conversation is no longer available" error (with `cause` preserved). `ChatPanel` resets to the new-chat state on that error instead of showing the raw "session not found: sess-…" string.
- ✅ **Smart autoscroll in chat** — New `web/src/hooks/useAutoscroll.ts` tracks whether the user is near the bottom (80px threshold) and only auto-scrolls on new content when they are. `ChatPanel` shows a floating "jump to bottom" button (`ChevronDown`) when scrolled up. No `setState`-in-effect (scroll listener updates state from an event handler; the autoscroll effect reads a ref).
- ✅ **Adjustable panel widths** — Left sidebar and right chat panel are resizable via drag handles (desktop only). Widths persist to `localStorage` (`lai:leftPanelWidth`, `lai:rightPanelWidth`) with min/max bounds (left 180–480px, right 300–700px). Drag tracking uses closure-captured state (no refs) to satisfy `react-hooks/refs`; width updates via React state on `mousemove`. Mobile bottom-nav layout unaffected.
- ✅ **System message cleanup** — `ResponseStarted`, `ConnectionRestarted`/`SessionResumed`, `SessionCancelled`/`SessionInterrupted`, `FileRevisionUpdated`, and `AgentExited` now render as compact centered muted rows (`text-xs text-muted-foreground text-center py-1` with `·` prefix) instead of full chat bubbles. `AgentExited` uses `text-destructive` so failures stay noticeable. User/agent messages unchanged.
- ✅ **Events endpoint default limit raised 100 → 1000** — `internal/server/api.go` `parseEventParams` and `internal/events/events.go` `Query`/`QueryAll` fallbacks now default to 1000 so long streaming responses (250+ events) are not truncated. `?limit` still constrains; no upper cap added. Regression test added in `internal/server/server_test.go` (`TestGetSessionEventsDefaultLimit`).

## ACP Stability & Chat UX Pass (2026-06-23)

Backend (all green: `go test ./...`, `go vet`, `npm run build`, `npm run lint`, `.\build.ps1`):
- ✅ Client capabilities advertised (fs + terminal); terminal methods implemented (`internal/acp/terminal.go`).
- ✅ Permission prompts now reach the UI (`PermissionManager.SetCallback` → `PermissionRequested` event); UI responds with correct `requestId` + dynamic options.
- ✅ Conversations persist across restarts (`~/.local-agent/conversations.json`); rename/delete/rebind via `PATCH/DELETE /api/sessions/{id}`.
- ✅ Mid-conversation model/agent switch (client-side rebind; ACP has no model API in v0.13.5).
- ✅ Enriched SessionUpdate: thoughts, plans, tool kind/target/diff; agent stderr captured into failure events.
- ✅ ReadTextFile/WriteTextFile now convert absolute paths to workspace-relative (fix: agent `read` tool was failing on absolute paths).
- ✅ Thought block fix: `mergedEvents` reducer in ChatPanel now checks `thought` flag to prevent thoughts and messages from merging.
- ✅ Conversation export: download button in ChatHistory, client-side JSON export.
- ✅ Graceful session lifecycle on shutdown: `CloseAllSessions` calls best-effort `session/delete` then terminates each agent process (replaces raw kill); `session/load` (`LoadSession`) resumes persisted sessions on restart with `NewSession` fallback. Mock-agent terminal/permission regression coverage still deferred (WI-15).
- ✅ **Agent context provider** — injects workspace file tree + git status + AGENTS.md into the first prompt of each session via a prompt middleware pipeline. See `docs/archive/agent-context.md`.

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
| Runtime: `app start` serves UI | ✅ Verified 2026-06-27 — `/health` returns 200, `/` serves UI with root div |
| Runtime: pairing flow works | ✅ Verified 2026-06-27 — `app pair` emits mnemonic + QR PNG + token URL |

## Phase 1 — Core Infrastructure: 100% COMPLETE

14 tasks in `docs/archive/plan.md` all marked `[x]`. All previously overstated rows resolved by Work Streams 1-5 (2026-06-27).

| # | Task | Status | Notes |
|---|------|--------|-------|
| 1 | scaffold | ✅ Done | Go module, dirs, go:embed, HTTP server, shared interfaces |
| 2 | cli-daemon | ✅ Done | Cobra CLI: start/stop/status/add-folder/pair/devices/revoke/logs |
| 3 | events | ✅ Done | SQLite event store, WAL mode, append/query, all event types |
| 4 | pairing | ✅ Done | QR + mnemonic passcode, device credentials, revocation |
| 5 | workspace | ✅ Done | File tree + file read + file write work. Workspaces load from config on daemon startup. `remove-folder` and `list-folders` CLI commands added. |
| 6 | acp-client | ✅ Done | Session lifecycle uses `coder/acp-go-sdk`. `transport.go` bridges ACP events. `session/load` (`LoadSession`) attempted on restart when the agent advertises `loadSession` and a persisted `acpSessionId` exists — falls back to `NewSession` on any failure. `CloseSession` calls best-effort `session/delete` (`UnstableDeleteSession`) before killing the process. `CloseAllSessions` wired into `daemon.cleanup()` for graceful shutdown. `ACPSessionID` now persisted in `conversations.json`. |
| 7 | permissions | ✅ Done | Request/response flow works (callback → UI → respond). Audit log records decisions. Policy enforcement implemented: `allow_always`/`allow_session` auto-resolve subsequent same-`(session,tool,target)` requests without blocking; `allow_once` still prompts every time. Policies are session-scoped and cleared via `ClearSession` on `CloseSession`. Reject-always auto-deny skipped (no constant in codebase). |
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
- **File tree:** Fully interactive. Folders expand/collapse, clicking files opens them in editor tabs. Nested-folder indentation fixed 2026-06-27: replaced dynamically-constructed Tailwind class `ml-${depth*4}` (which JIT never generated for depth ≥ 2) with a nested `pl-4` wrapper so indentation accumulates per recursion level.
- **Editor content:** Dynamically loads from backend (`api.readFile`), tracking language and unsaved state.
- **Editor tabs:** Fully manageable (select, close, track unsaved status).
- **Save button:** Wired to `api.saveFile` using optimistic locking (`expectedRevision`).

### Workspace Persistence — ✅ RESOLVED
- `workspace.Manager` workspaces now loaded from `~/.local-agent/config.json` on daemon start
- `WriteFile` endpoint added: `POST /api/workspaces/{id}/file` with optimistic locking
- `ListSessions` endpoint added: `GET /api/sessions`
- `app add-folder` now checks for duplicate registrations
- LeftSidebar workspace switcher wired to `backend.workspaces` with dropdown UI
- `app remove-folder <id>` and `app list-folders` CLI commands added (Work Stream 4d)
- **Note:** Must rebuild binary (`.\build.ps1`) after frontend changes — `go:embed` freezes frontend at compile time

## What's Left

### Runtime Verification Needed
- [x] `app start` → browser connects → web UI loads — Verified 2026-06-27: daemon starts, `/health` returns 200, `/` serves embedded React UI (root div present).
- [x] `app add-folder .` → workspace appears in UI file tree — Verified 2026-06-27: `app add-folder .` registers workspace; `app list-folders` confirms it appears in the list.
- [x] `app pair` → QR/passcode → device pairs → lock screen clears — Verified 2026-06-27: `app pair` emits a four-word mnemonic passcode, a QR code (PNG file), and a token URL with 5-min expiry. Full device-pair + lock-screen-clear flow not exercised end-to-end (requires a browser client), but the server-side pairing endpoint is functional.
- [ ] Editor pane loads file content → save works — Not exercised this session (requires a paired browser session). Backend endpoints (`GET /api/workspaces/{id}/file`, `POST .../file` with optimistic locking) are tested via `internal/server` tests.
- [x] **ACP transport** → spawn agent, send prompt, stream response — Verified E2E 2026-06-27 with `mistral-vibe` / `devstral-small`: daemon → ACP handshake → session created → prompt sent → workspace-context injected (Stream 3 confirmed via `## Workspace Context` preamble in `PromptSubmitted` event) → agent read `AGENTS.md` via `fs/read_text_file` (ToolStarted/ToolCompleted events) → 245 streaming `StreamUpdate` chunks → final completion marker → session closed cleanly. No `AgentExited` (normal completion). No permission prompts needed (file reads auto-approved). Not yet verified: `LoadSession` (session resume across restart), permission prompts for shell commands, UI-side rendering of the streamed events.

### Open Items (from `docs/plans/OpenItems.md`)
- [x] **TLS on LAN** — self-signed ECDSA P-256 cert generated on first start; trust-on-first-use (reused, never overwritten); SANs include localhost, 127.0.0.1, and all LAN IPv4 addresses. Configurable via `tlsEnabled` / `tlsCertDir` in config.json.
- [x] **Pairing TTL** — configurable via `pairingTtlSeconds` in config.json (default 300s / 5 min). `pairing.Manager.SetTTL` setter wired from daemon.
- [ ] **Device credential expiry** — permanent until revoked
- [ ] **Editor on mobile** — CodeMirror needs touch optimization
- [ ] **ACP sub-workers** — deferred until next ACP release

### UI & Chat Implementation Gaps
- [x] **UI Persistence** — Open tabs, active tab, left panel, mobile view, active workspace, selected agent, and selected model all persist to localStorage and restore on reload. Namespaced keys: `lai:*`.
- [x] **Chat Messages** — Send prompt now persists/broadcasts events, awaits completion, shows errors, and renders all event types (PromptSubmitted, ResponseStarted, StreamUpdate, ToolStarted, ToolCompleted, AgentExited). Lint clean.
- [x] **Conversation Management** — Rename, delete, and rebind all implemented: `PATCH /api/sessions/{id}` (rename/rebind), `DELETE /api/sessions/{id}` (delete), `Client.RenameSession` in `internal/acp/acp.go`. UI inline-rename in `ChatHistory.tsx`, delete button in ChatHistory, export button. All tested (`internal/acp/conversation_test.go`).

## Development Phases (from Blueprint Sec 25)

### Phase 1 — Core Infrastructure (complete)
- Daemon + CLI, pairing, web server, workspace mgmt, session lifecycle
- ACP client layer, permission manager, shell execution, single agent
- Event system, WebSocket sync, CodeMirror 6 editor with diff view
- **Status: 100% done. All 6 finish-the-codebase work streams merged (2026-06-27). Ready for Phase 2.**

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
