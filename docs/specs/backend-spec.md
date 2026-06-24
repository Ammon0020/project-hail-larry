# Backend Specification — ACP Communication & Conversation Service

**Status:** Draft v1 (2026-06-23)
**Goal:** A stable backend that communicates with Codex, Mistral Vibe, and Claude
Code over ACP (`coder/acp-go-sdk` v0.13.5) and drives the web UI defined in
`docs/specs/ui-spec.md`. Use ACP and the SDK wherever possible; only add
client-side concepts where the protocol has gaps.

---

## 1. Principles

- **ACP is the only agent protocol.** No per-agent code beyond launch command +
  model discovery. All runtime interaction is ACP.
- **Client owns the environment.** The daemon executes filesystem + shell on the
  agent's behalf and owns permissions, per ACP's client-server model.
- **Event log is the source of truth.** Every meaningful state change is an
  append-only event; the UI is derived from it. Persisted in SQLite.
- **Fail loudly.** Every error becomes an event and/or an HTTP error with a
  human-readable message. No silent drops.

---

## 2. ACP Capabilities (fixes Issue #1)

During `initialize`, advertise what we actually support:

```go
ClientCapabilities{
    Fs: FileSystemCapabilities{ ReadTextFile: true, WriteTextFile: true },
    Terminal: true, // only after terminal methods implemented (§4)
}
```

Apply to both the real transport and the autodetect probe (probe may keep
`Terminal:false` since it only needs initialize + providers/list).

---

## 3. Session Update Handling (fixes Issue #3 & #4)

`acpClientImpl.SessionUpdate` must translate every relevant ACP update into an
`interfaces.Event` (no silent drops):

| ACP update | Event | Fields populated |
|---|---|---|
| `AgentMessageChunk` | `StreamUpdate` (role=agent, streaming) | `Content` |
| `AgentThoughtChunk` | `StreamUpdate` (role=agent, `Thought=true`) | `Content` |
| `ToolCall` | `ToolStarted` | `Tool`(title), `ToolKind`, `ToolCallID`, `Target`(first location), `Command`(rawInput if string), `Status` |
| `ToolCallUpdate` | `ToolCompleted` (or progress) | `ToolCallID`, `Status`, `Summary`, `Content`(diff/text), `Target` |
| `Plan` / `PlanUpdate` | `Plan` | serialized entries (title + status) |
| `UserMessageChunk` | ignored (already echoed) | — |
| `CurrentModeUpdate` / others | optional system note | — |

New optional fields on `interfaces.Event`: `ToolKind`, `ToolCallID`, `Thought`,
`ExitCode`. Keep them `omitempty` so existing consumers/tests are unaffected.

---

## 4. Terminal Methods (fixes Issue #2)

Implement the five ACP terminal methods on `acpClientImpl`, backed by
`internal/shell`. Maintain `map[string]*terminalEntry` keyed by a generated
terminal id, guarded by a mutex.

- `CreateTerminal` — build command string from `Command`+`Args`, run via
  `shell.Executor.RunAsync` in the session workspace; stream stdout/stderr into a
  buffer and emit `ShellCommandStarted` + `ShellOutputStreamed` events; return a
  generated `TerminalId` immediately (non-blocking).
- `TerminalOutput` — return buffered output + `ExitStatus` when finished, with
  `Truncated` honouring `OutputByteLimit`.
- `WaitForTerminalExit` — block until process exits; return exit code/signal.
- `KillTerminal` — kill process, keep the entry.
- `ReleaseTerminal` — kill if running, delete the entry; emit
  `ShellCommandCompleted`.

Each terminal entry tracks: command, workspace, buffer (capped), done channel,
exit code, cancel func. Output retained up to `OutputByteLimit` (default cap,
truncate from front on a rune boundary).

Only after this works, set `Terminal: true` in client capabilities.

---

## 5. Permissions (fixes the broken UI path — highest priority)

Current bug: `PermissionManager.Request()` blocks on a channel but the request is
never broadcast, so the UI never prompts.

New flow:
1. `acpClientImpl.RequestPermission` builds an `interfaces.PermissionRequest`
   with a generated `ID`, `SessionID` (our conversation id), `Tool`, `Command`/
   `Target`, and the agent-provided `Options` (mapped from ACP option kinds).
2. `PermissionManager.Request` stores the pending request **and notifies a
   callback** (`OnRequest`) so the server emits a `PermissionRequested` event
   (carrying `requestId`, `sessionId`, `tool`, `command`, `target`, `options`)
   and broadcasts it over WebSocket.
3. UI responds → `POST /api/permissions/{requestId}/respond {decision}` →
   `PermissionManager.Respond` → unblocks the channel; server emits
   `PermissionGranted`/`PermissionDenied` (carrying the same `requestId`).
4. `acpClientImpl.RequestPermission` maps the chosen `PermissionDecision` back to
   the ACP `OptionId` and returns it (deny → `Cancelled` outcome).
5. On context timeout, the request resolves to `deny` and emits a denied event.
6. `GET /api/permissions/pending` re-presents outstanding prompts after reconnect.

`PermissionManager` gains a `SetCallback(func(PermissionRequest))` hook; the
server registers it (like `ACPClient.SetCallbacks`). The `Event` struct gains a
`RequestID` field (omitempty).

Decision→option mapping must include the ACP option ids the agent sent; we keep a
per-request `map[PermissionDecision]optionId` so `Respond` can resolve it.

---

## 6. Conversations (remember / rename / delete / model-switch)

ACP sessions are transient. Introduce a persisted **conversation** record so the
UI can remember, rename, delete, and re-bind model/agent.

### 6.1 Storage
Add a `conversations` table (same SQLite db as events) or a JSON store:

```
conversation:
  id            string  (our session id, "sess-…")
  name          string  (user-editable; default from first prompt or "New chat")
  agent_id      string
  model_id      string
  workspace_id  string
  status        string  (created|running|completed|failed|interrupted)
  created_at    time
  updated_at    time
```

`acp.Session` (in-memory, holds the live transport + acp session id) references a
conversation id. On daemon start, conversations are loaded from storage; their
live ACP sessions are **not** auto-started (lazy: started on next prompt or model
bind). Events already persist and are keyed by conversation id.

### 6.2 Lifecycle
- **Create** (`POST /api/sessions`): create conversation + start ACP session
  immediately (so capabilities/handshake validated). Persist record.
- **Send prompt**: if the conversation has no live ACP session (e.g. after
  restart), transparently start one before prompting.
- **Rename** (`PATCH /api/sessions/{id}` `{name}`): update record; emit nothing
  or a lightweight event; return updated record.
- **Delete** (`DELETE /api/sessions/{id}`): close live ACP session, delete record.
  Events: retained by default (history) but hidden from list; provide
  `?purge=true` to also delete events. (Default = retain.)
- **List** (`GET /api/sessions`): from persisted records, newest-first, with
  `name`, `agentId`, `modelId`, `status`, `updatedAt`.

### 6.3 Model / agent switch (client-side re-bind)
`PATCH /api/sessions/{id}` `{agentId?, modelId?}`:
1. Validate agent+model exist.
2. If a turn is running, cancel it first.
3. Close the old ACP session/transport; create a new one for the new agent/model.
4. Update the conversation record (agent_id/model_id) and persist.
5. Emit a system event (e.g. `StreamUpdate` role=system or a dedicated
   `ConnectionRestarted`) noting the switch so the UI shows it inline.
History (events) is preserved because it is keyed by the unchanged conversation id.

> Because there is no ACP model API, the new agent starts a fresh ACP session and
> will not have the prior turns' in-agent context. The UI shows prior history; the
> agent's own memory restarts. This is the honest, documented limitation. (Future:
> replay history via `session/load` or by re-sending a summary.)

---

## 7. Session close/resume (Issue #6, partial)

- `Transport.Close()` should attempt `conn.CloseSession()` (best-effort) before
  killing the process, so agents can clean up.
- `LoadSession`/`ResumeSession` are available in the SDK but deferred unless the
  agent advertises support; not required for v1.

---

## 8. Agent stderr (Issue #7)

Replace `cmd.Stderr = os.Stderr` with a per-session capped ring buffer. On agent
failure, include the tail of stderr in the `AgentExited` event summary so the UI
can show *why* it died. Keeps daemon logs clean.

---

## 9. Model discovery / providers (Issue #9)

Keep autodetect at startup (PATH probe → `UnstableListProviders` → config file →
fallback). Optionally refresh on `POST /api/agents/autodetect` (already exists).
No per-session providers call required for v1.

---

## 10. REST / WS Surface (target)

| Method | Path | Purpose | Change |
|---|---|---|---|
| GET | `/api/agents` | list agents+models | existing |
| POST | `/api/agents` | upsert agent | existing |
| DELETE | `/api/agents/{id}` | remove agent | existing |
| POST | `/api/agents/autodetect` | re-probe | existing |
| GET | `/api/sessions` | list conversations (rich) | **enrich** |
| POST | `/api/sessions` | create conversation | existing |
| PATCH | `/api/sessions/{id}` | rename and/or re-bind agent/model | **new** |
| DELETE | `/api/sessions/{id}` | delete conversation | existing (wire UI) |
| POST | `/api/sessions/{id}/prompt` | send prompt (lazy-start) | **enhance** |
| POST | `/api/sessions/{id}/cancel` | stop current turn | existing |
| GET | `/api/events[/{id}]` | event history / per-conversation | existing |
| GET | `/api/permissions/pending` | re-present prompts | existing |
| POST | `/api/permissions/{id}/respond` | answer prompt | existing (fix UI) |
| WS | `/ws` | event stream | existing |

All handlers return `{ "error": "…" }` with an appropriate status on failure.

---

## 11. Event Schema Additions

`interfaces.Event` new optional fields (all `omitempty`, JSON-tagged):
`RequestID string`, `ToolKind string`, `ToolCallID string`, `Thought bool`,
`ExitCode *int`. Existing fields unchanged. SQLite event store must persist and
return the new columns (add columns with migration / `ALTER TABLE … ADD COLUMN`
guarded by existence check, or widen the serialized JSON blob if events are
stored as JSON).

---

## 12. Testing Requirements

- Extend `cmd/mockagent` to exercise: capabilities echo, a tool call with
  kind+locations, a permission request, a terminal create+output+wait, and a
  thought + plan update.
- Unit tests:
  - permissions: `Request` notifies callback; `Respond` resolves; timeout denies.
  - terminals: create→output→wait→release happy path + kill.
  - conversations: create/list/rename/delete/persist-reload; model re-bind keeps id.
  - session update translation: each ACP update → expected event fields.
- Integration (`internal/acp/integration_test.go`): full flow with the updated
  mock agent including a permission round-trip and a terminal command.
- Keep real-agent tests skip-by-default (`ACP_TEST_REAL=1`).
- `go test ./...`, `go vet ./...`, `golangci-lint`, `npm run build`, `npm run lint`, `.\build.ps1` all green.
