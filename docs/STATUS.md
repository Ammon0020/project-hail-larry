# Project Status — Local Agent Interface

> Last updated: 2026-06-30. Source of truth for task-level status.
> See `docs/plans/Blueprint.md` for architecture, `docs/plans/execution-plan.md` for work streams.

## Recent Fixes (2026-06-30)

- **Permission prompt label hardening** — Some agents set a permission request's `ToolCall.Title` to an opaque tool-call ID (e.g. Claude `toolu_…`, OpenAI `call_…`, UUIDs, or random tokens like `muNNhDHjd`), which surfaced as "Permission Required / <id>". The previous heuristic only caught separator-free tokens and wrongly treated any `_`/`-` value as a real label. Now `looksLikeRawID` (backend `internal/acp/transport.go`, mirrored in frontend `ChatMessageItem.tsx`) recognizes ID-prefixed tokens, UUIDs, and long hex, falling back to a kind-derived label ("Run command", "Edit file", …). Covered by `TestLooksLikeRawID`.
- **Cross-platform test fix** — `TestExpandPathWindowsEnv` compared against `filepath.Join` (OS-dependent separator) and failed on Linux/macOS even though `expandWindowsEnv` only substitutes `%VAR%`. Expectation now uses the literal backslash path so it passes on all platforms.
- **Linux/macOS build script** — Added `build.sh` (counterpart to `build.ps1`): builds the frontend, re-embeds `internal/server/dist`, and compiles/installs the Go binary. Note: `internal/server/dist/` is gitignored, so a frontend build (`npm install && npm run build`) is required before `go build` can embed assets.

## What Works

- **Daemon + CLI** — start/stop/status/add-folder/pair/devices/revoke/logs. TLS support, config persistence. `remove-folder` and `list-folders` functional.
- **ACP client** — full Agent Client Protocol via `coder/acp-go-sdk`. Session lifecycle, streaming, enriched tool calls, thoughts/plans, permission policies, terminal support properly wired to shell executor. Verified E2E.
- **Pairing** — QR + mnemonic passcode, device credentials, revocation, and configurable TTL.
- **WebSocket sync** — real-time event broadcast, reconnection sync, keepalive pings, loopback auth bypass.
- **Permissions** — request/response flow, `allow_always`/`allow_session` policies (shell commands keyed by command text, not just session), audit log.
- **Event store** — SQLite WAL mode, append-only, query/replay. Busy timeout + connection pool configured.
- **File sync** — revision tracking (content hash), three-way merge, per-file locking, bounded LRU cache.
- **Frontend shell** — React 19 + Vite 8 + Tailwind v4 + shadcn/ui. Desktop/mobile layouts, chat, session list, file tree, editor tabs.
- **Chat** — streaming responses, markdown rendering, thoughts/plans/tool cards, conversation rename/delete/rebind, export, smart autoscroll.
- **Security hardening** — auth middleware on all API routes, WebSocket auth, ACP terminal cwd validation, workspace map race fix, sync/ACP/shell goroutine leak fixes, CLI TLS.

## TODO

### High Priority

- [x] **File saving fixed** — Root cause: content-hash revisions were 64-bit, exceeding JS `Number.MAX_SAFE_INTEGER` (2^53-1). `JSON.parse` silently rounded them, causing every save to get 409 Conflict. Fixed by reducing `contentRevision` to 48 bits (within JS safe range). Verified: read → save → 200 OK.
- [x] **CodeMirror 6 editor upgraded** — Added: language auto-detection (JS/TS/JSX/TSX, CSS/SCSS/Less, HTML/XML/SVG, Python, Markdown with nested code block highlighting via `@codemirror/lang-markdown` + `@codemirror/language-data`), search (Ctrl+F), autocompletion, bracket matching, code folding, `defaultKeymap` + `historyKeymap` + `indentWithTab`, active line/gutter highlighting, draw selection, rectangular selection, Ctrl+S keybinding, line wrapping toggle. Fixed: editor now fills container height (parent `overflow: hidden`, `.cm-editor`/`.cm-scroller` `height: 100%`, `.cm-content` `paddingBottom: 50vh`) so clicking below the last line works. Still missing: `@codemirror/merge` for diff/merge view (not installed) and some kind of WYSIWYG style markdown viewer, obsidian style, with live rendering. Should show symbols while you're typing them or your cursor is on the line but hide them when you leave the line. 
- [x] **Workspace switching with active agent sessions** — Implemented the "show all sessions" model: sessions keep running when switching workspaces; the chat history popout lists sessions from ALL workspaces with a compact workspace badge next to each session name and an optional workspace filter `<select>` ("All Workspaces" + each registered workspace). Added `workspace` field to frontend `SessionInfo`/`Session` types (backend already sends it). Badge uses semantic Tailwind tokens and is hidden for legacy sessions with no workspace; filter still works with 0 workspaces.

### Medium Priority

- [x] **Event-based system message framework** — Externalized the prompt middleware pipeline's header strings and numeric limits to `configs/system-messages.json` (`SystemMessages` in `internal/acp/messages.go`, with `DefaultSystemMessages` fallback). Refactored `FirstPromptContextMiddleware` to use templates. Added three new per-prompt context providers in `internal/acp/providers.go`: `TimeMiddleware` (current time, every prompt), `OpenFilesMiddleware` (currently open files, every prompt), `RecentEditsMiddleware` (recently edited files, every prompt). Added `OpenFilesTracker` (in-memory, thread-safe) holding frontend-reported editor state, consulted by the open-files/recent-edits middlewares (skip injection when empty). Added `POST /api/sessions/{id}/context` REST endpoint (auth-wrapped) for the frontend to report `{ openFiles, recentEdits }`. Pipeline wired in `internal/daemon/daemon.go`. Frontend integration (sending open files to the backend) is a separate bite — the tracker starts empty and middlewares gracefully skip until state is reported. Design reference: `docs/plans/agent-context.md`.

- [x] **File search** — Implemented. REST endpoint `GET /api/workspaces/{id}/search` (regex pattern, ignoreCase, maxResults, filePattern, contextLines) backed by `internal/search` which uses ripgrep when on PATH and falls back to a Go-native walker (filepath.WalkDir + bufio.Scanner + regexp) otherwise. Skips hidden files/dirs and common noise dirs (.git, node_modules, vendor, dist, build, .next, target) to match the file tree; binary files detected via null-byte sampling. SearchPanel wired to the endpoint with a 300ms debounce, stale-request cancellation, results grouped by file, match highlighting, and an ignore-case toggle. Clicking a result opens the file in the editor; line-jump to the match is deferred (see docs/known-issues.md).
- [ ] **Device credential expiry** — Permanent until revoked. Need to decide: time-limited or permanent?
- [x] **Reconnection behavior** — Phone drops Wi-Fi mid-session; in-flight permission prompts need handling. Implemented: (1) `permissions.Manager.CleanupStale()` denies and removes pending prompts older than 5min (`pendingRequestTimeout`), unblocking the agent goroutine that was waiting in `Request`; called at the start of `GetPending()` so a reconnecting client never receives a stale prompt list. (2) `ClearSession` (prior subagent) denies pending prompts for a closed session and bounds the audit log; already wired in `acp.CloseSession`. (3) Frontend `useBackend` exposes a `reconnecting` flag (true only after a prior successful `ws.onopen`, so cold-load failures don't flash a banner) and `App.tsx` renders a thin "Reconnecting…" banner with a pulsing dot (semantic Tailwind tokens) above the main shell while the WebSocket is down; on reconnect `loadPendingPermissions`/`loadSessions`/`loadEvents` re-sync state. Remaining: no transient "N pending prompts need attention" toast on reconnect (deferred — the existing pending-permissions UI in ChatPanel already re-surfaces prompts, so a toast would duplicate it).
- [x] **Conversation transfer on harness switch** — When a user rebinds a conversation to a different agent/model mid-chat, the prior conversation history is now exported as a markdown transcript and injected as context for the new agent's first prompt (truncated to `maxContextBytes`). `ExportConversation` in `internal/acp/conversation.go` reads the session's events from the event store and renders `**User:**`/`**Assistant:**` turns with compact `[Tool: {name}]` summaries, skipping internal events (ResponseStarted, ConnectionRestarted, FileWritten, permission/shell/plan events). `ConversationTransferMiddleware` queues the transcript via `SetTransfer` (called by `RebindSession`) and injects it under the `ConversationTransferHeader` ("## Previous Conversation (transferred from {agentName})") on `PromptCount == 0`, then clears the queue. `RebindSession` now captures the old agent name, exports the transcript (best-effort on error), resets the per-session prompt counter so first-prompt middlewares fire again, and queues the transfer. Wired in `internal/daemon/daemon.go`: the middleware is added to the pipeline after `FirstPromptContextMiddleware` (workspace context first, then transfer), and the client gets the event store + transfer middleware via `SetEventStore`/`SetConversationTransfer`. The `ConnectionRestarted` event now notes the export. No REST/frontend changes needed — the rebind endpoint already passed the request context, and the event stream carries the transfer notice. Tests in `internal/acp/conversation_test.go` cover export formatting, truncation, nil/empty stores, middleware injection/clearing semantics, and the `RebindSession`→queue→inject flow.
- [ ] **Live agent change detection in editor** — When the agent modifies a file being edited, the editor should show an indicator without forcing a reload. Conflict resolution triggers only on save (Blueprint Sec 14). External file changes (e.g. edited in Notepad) should also trigger a UI update if the user has no unsaved changes.
- [ ] **Remaining review findings** (in `review/2026-06-27/`, not in `implemented/`):
  - 8 `web-*` frontend findings remain (deferred — need larger refactor or design decision; see `docs/known-issues.md`). 17 of 25 were fixed and moved to `implemented/`.

  Done (moved to `implemented/`): `go-core-config-data-race`, `go-core-pair-initiate-swallows-decode-error`, `go-permissions-audit-log-unbounded`, `go-permissions-clearsession-no-cancel`.

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
