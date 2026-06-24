# Implementation Plan — ACP Stability & Chat UX

**Tracks:** `docs/specs/backend-spec.md` + `docs/specs/ui-spec.md`.
**Definition of done:** `go test ./...`, `go vet ./...`, `golangci-lint`,
`npm run lint`, `npm run build`, and `.\build.ps1` all pass.

Work is grouped into independent-ish work items. Each item lists files and a
verification step. Order is chosen so the build stays green at every step.

---

## WI-1 — Event schema additions (foundation)
**Files:** `internal/interfaces/interfaces.go`, `internal/events/events.go`
- Add `RequestID`, `ToolKind`, `ToolCallID`, `Thought`, `ExitCode *int` to `Event`.
- Extend `eventPayload` marshal/scan (JSON blob — no SQL migration).
**Verify:** `go test ./internal/events/...`

## WI-2 — ACP client capabilities (Issue #1)
**Files:** `internal/acp/transport.go` (and autodetect stays minimal)
- Advertise `Fs.ReadTextFile/WriteTextFile = true`. `Terminal` set true in WI-4.
**Verify:** existing acp tests.

## WI-3 — Permission event wiring (highest priority, fixes broken UI)
**Files:** `internal/permissions/permissions.go`, `internal/server/server.go`,
`internal/server/api.go`, `internal/acp/transport.go`, `internal/interfaces/interfaces.go`
- `PermissionManager` gains `SetCallback(func(PermissionRequest))`; call it in
  `Request` before blocking.
- Server registers callback → emits `PermissionRequested` event (requestId,
  sessionId, tool, command, target, options) + broadcasts.
- `Respond` path emits `PermissionGranted`/`PermissionDenied` with `RequestID`.
- `transport.RequestPermission` keeps option→optionId map, sets `req.ID`,
  populates command/target, maps deny→Cancelled outcome.
**Verify:** new `permissions` test (callback fires, respond resolves, timeout denies).

## WI-4 — Terminal methods (Issue #2)
**Files:** `internal/acp/transport.go` (+ maybe `internal/acp/terminal.go`)
- `terminalEntry` map + mutex; wire to `shell.Executor.RunAsync`.
- Implement Create/Output/WaitForExit/Kill/Release; emit shell events.
- Set `Terminal: true` capability.
**Verify:** new terminal unit test; mock agent terminal flow.

## WI-5 — Session update enrichment (Issues #3, #4)
**Files:** `internal/acp/transport.go`
- Handle `AgentThoughtChunk` (Thought=true), enrich `ToolCall`/`ToolCallUpdate`
  with kind/locations/rawInput/summary, handle `Plan`/`PlanUpdate`.
**Verify:** unit test on a synthetic `SessionNotification`.

## WI-6 — Conversation persistence + rename/delete/rebind
**Files:** `internal/acp/acp.go`, new `internal/acp/store.go` (or reuse config),
`internal/server/api.go`, `internal/server/server.go`, `internal/interfaces/interfaces.go`
- Persist conversation records (id, name, agentId, modelId, workspaceId, status,
  timestamps) to a JSON file in the data dir (simple, testable) or a SQLite table.
- `ListSessions` returns persisted records (rich), newest-first.
- `CreateSession` persists; default name "New chat".
- Prompt path lazily (re)starts an ACP session if none live.
- Add `PATCH /api/sessions/{id}` for `{name}` and/or `{agentId, modelId}` re-bind
  (cancel running turn, close+recreate transport, keep id, emit switch note).
- `DELETE` closes + removes record (retain events by default).
- First user prompt sets conversation name if still default.
**Verify:** `acp` conversation tests (create/list/rename/delete/reload/rebind).

## WI-7 — Agent stderr capture (Issue #7)
**Files:** `internal/acp/transport.go`
- Replace `os.Stderr` with capped ring buffer; include tail in `AgentExited`.
**Verify:** build + existing tests.

## WI-8 — Frontend: API + types
**Files:** `web/src/lib/api.ts`, `web/src/types/index.ts`
- Add `patchSession(id, {name?, agentId?, modelId?})`, surface `closeSession`.
- Extend `AppEvent` + `SessionInfo` (agentId, modelId, updatedAt) and event fields
  (requestId, toolKind, thought, exitCode).

## WI-9 — Frontend: permission card fix
**Files:** `web/src/components/ChatMessageItem.tsx`, `ChatPanel.tsx`, `App.tsx`, `useBackend.ts`
- Render one button per `options`; respond with `requestId` + decision string.
- Resolve cards on Granted/Denied.

## WI-10 — Frontend: connection + failure UX
**Files:** `web/src/components/ChatPanel.tsx`, `useBackend.ts`, maybe new `ConnectionBanner.tsx`
- Show connection dot/banner; on reconnect re-sync events `after=lastId` + refetch
  pending permissions; ret[r]y affordances; agent-missing CTA; preserve composer text.

## WI-11 — Frontend: conversation management UI
**Files:** `web/src/components/ChatHistory.tsx`, `ChatPanel.tsx`, `App.tsx`
- Rename (inline), delete (confirm), select; show model/agent per conversation.
- Persist active conversation + model in `localStorage`.

## WI-12 — Frontend: model/agent switch on existing conversation
**Files:** `ChatPanel.tsx`, `App.tsx`, `useBackend.ts`
- Changing selector on active conversation calls `patchSession`; shows inline note;
  guards while running.

## WI-13 — Frontend: render remaining events
**Files:** `web/src/components/ChatMessageItem.tsx`
- Thoughts, plans, shell command cards, file-revision notes, cancelled/system notes.

## WI-14 — Composer cancel/stop
**Files:** `ChatPanel.tsx`, `App.tsx`
- Stop button while running → `cancelSession`.

## WI-15 — Mock agent + integration tests
**Files:** `cmd/mockagent/main.go`, `internal/acp/integration_test.go`, `internal/acp/*_test.go`
- Exercise capabilities, tool call w/ kind, permission round-trip, terminal,
  thought + plan.

## WI-16 — Verify + docs
- Run all gates; update `docs/STATUS.md` and `docs/acp-shortcomings.md` (mark fixed).

---

## Risk notes
- Model switch loses in-agent context (no ACP model API). Documented; UI shows a note.
- SQLite event payload is JSON → schema additions are backward compatible.
- Keep changes additive to avoid breaking existing passing tests.
