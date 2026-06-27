# Project Status — Local Agent Interface

> Last updated: 2026-06-27. Source of truth for task-level status.
> See `docs/plans/Blueprint.md` for architecture, `docs/plans/execution-plan.md` for work streams.

## What Works

- **Daemon + CLI** — start/stop/status/add-folder/pair/devices/revoke/logs. TLS support, config persistence.
- **ACP client** — full Agent Client Protocol via `coder/acp-go-sdk`. Session lifecycle, streaming, tool calls, permission prompts, terminal support. Verified E2E with `mistral-vibe`/`devstral-small`.
- **Pairing** — QR + mnemonic passcode, device credentials (hashed at rest, persisted to disk), revocation. Rate-limited with constant-time compares.
- **WebSocket sync** — real-time event broadcast, reconnection sync, keepalive pings, loopback auth bypass.
- **Permissions** — request/response flow, `allow_always`/`allow_session` policies (shell commands keyed by command text, not just session), audit log.
- **Event store** — SQLite WAL mode, append-only, query/replay. Busy timeout + connection pool configured.
- **File sync** — revision tracking (content hash), three-way merge, per-file locking, bounded LRU cache.
- **Frontend shell** — React 19 + Vite 8 + Tailwind v4 + shadcn/ui. Desktop/mobile layouts, chat, session list, file tree, editor tabs.
- **Chat** — streaming responses, markdown rendering, thoughts/plans/tool cards, conversation rename/delete/rebind, export, smart autoscroll.
- **Security hardening** — auth middleware on all API routes, WebSocket auth, ACP terminal cwd validation, workspace map race fix, sync/ACP/shell goroutine leak fixes, CLI TLS.

## TODO

### High Priority

- [ ] **File saving not functional** — Save button and `api.saveFile` endpoint are wired (`App.tsx` `handleSave` → `backend.saveFile` → `POST /api/workspaces/{id}/file` with optimistic locking), but saving does not work in practice. Needs debugging.
- [ ] **CodeMirror 6 editor is minimal** — Only `@codemirror/lang-javascript` + `@codemirror/theme-one-dark` wired. CSS/HTML/Python packs installed but unused. Missing: language auto-detection, bracket matching, autocompletion, search (`@codemirror/search` installed but not enabled), code folding, linting, keybindings (Ctrl+S, Ctrl+F), line wrapping, `@codemirror/merge` for diff/merge view. See `docs/development/TechStack.md` and `docs/plans/Blueprint.md` Sec "Editor and File Viewing".
- [ ] **Workspace switching with active agent sessions** — Conversations are linked to a workspace (`SessionInfo.workspaceId`), but the coordination model for switching workspaces while an agent session is active is not designed. Open questions: Does switching pause/hide the current session? Can a session span multiple workspaces? Should chat show sessions from all workspaces or only the active one? Needs a design decision before implementation.

### Medium Priority

- [ ] **File search** — Search panel (`web/src/components/SearchPanel.tsx`) is static/non-functional. No backend search endpoint exists.
- [ ] **Device credential expiry** — Permanent until revoked. Need to decide: time-limited or permanent?
- [ ] **Reconnection behavior** — Phone drops Wi-Fi mid-session; in-flight permission prompts need handling.
- [ ] **Live agent change detection in editor** — When the agent modifies a file being edited, the editor should show an indicator without forcing a reload. Conflict resolution triggers only on save (Blueprint Sec 14). External file changes (e.g. edited in Notepad) should also trigger a UI update if the user has no unsaved changes.
- [ ] **Remaining review findings** (in `review/2026-06-27/`, not in `implemented/`):
  - `go-core-config-data-race.md` — `handleUpsertAgent`/`handleDeleteAgent` mutate `Config.Agents` with no mutex
  - `go-core-pair-initiate-swallows-decode-error.md` — `handlePairInitiate` swallows decode errors, uses hardcoded `localhost:7337`
  - `go-permissions-audit-log-unbounded.md` — audit log grows without bound
  - `go-permissions-clearsession-no-cancel.md` — `ClearSession` leaves pending requests blocked
  - 25 `web-*` frontend findings (inline CSS, raw colors, dead UI, accessibility, type safety, etc.)

### Lower Priority / Future

- [ ] **Editor on mobile** — CodeMirror needs touch optimization
- [ ] **Image upload flow** — How whiteboard photos / images reach the agent via ACP
- [ ] **Multi-user vs multi-device** — One user's devices only, or can multiple people pair to the same daemon?
- [ ] **ACP sub-workers** — Deferred until next ACP release
- [ ] **Team collaboration** — Shared workspaces, multiple operators
- [ ] **Session replay** — Implementation details
- [ ] **Developer terminal UI** — Optional Phase 3 power-user feature

## Architecture Overview

```
cmd/app/main.go          → Cobra CLI (start/stop/status/pair/...)
internal/daemon/         → Lifecycle, wires all managers into server
internal/server/         → HTTP server, go:embed frontend, REST API, /ws
internal/events/         → SQLite event store (WAL, append-only)
internal/pairing/        → QR + mnemonic pairing, device credentials
internal/workspace/      → Registration, file tree, git info
internal/acp/            → ACP client using coder/acp-go-sdk, session lifecycle
internal/permissions/    → Permission request/response, policies
internal/sync/           → WebSocket hub, broadcast, reconnection
internal/files/          → Revision tracking, three-way merge
internal/shell/          → Workspace-scoped subprocess runner
internal/interfaces/     → Shared Go interfaces (EventStore, etc.)
web/                     → React 19 + Vite 8 + Tailwind v4 + shadcn/ui
  src/hooks/useBackend.ts → Real backend hook (REST + WebSocket)
  src/lib/api.ts          → REST API client
  src/components/         → UI components
```

## Verification

| Check | Status |
|-------|--------|
| `go build ./...` | ✅ Pass |
| `go test ./...` | ✅ All packages pass |
| `go vet ./...` | ✅ Pass |
| `npm run build` | ✅ Pass (web/) |
| `.\build.ps1` | ✅ Pass (frontend embedded) |
| Runtime: `app start` serves UI | ✅ Verified 2026-06-27 |
| Runtime: ACP E2E | ✅ Verified 2026-06-27 |

## Development Phases

- **Phase 1 — Core Infrastructure:** Complete. All 14 tasks done.
- **Phase 2 — Multi-Agent Support:** Not started. Agent registry, capability negotiation, multiple simultaneous workers, session resume, enhanced diagnostics.
- **Phase 3 — Advanced Features:** Not started. MCP management, multi-client collaboration, plugin architecture, session replay, developer terminal, UI polish.

## Key Decisions

- Go module: `github.com/adama/local-agent`
- Frontend dir: `web/` (not `frontend/`)
- Dark theme default; Tailwind v4 `@theme inline` pattern
- Event-driven UI: all rendering derives from `AppEvent[]` stream
- SQLite via `modernc.org/sqlite` (pure-Go, no CGO)
- Must rebuild binary (`.\build.ps1`) after frontend changes — `go:embed` freezes frontend at compile time
