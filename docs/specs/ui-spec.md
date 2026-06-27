# UI Specification — Agent Chat & Conversation Management

> **Status update 2026-06-27:** Features marked ✅ are shipped; ⏳ are backlog. See `docs/STATUS.md` for details.

**Status:** Draft v1 (2026-06-23)
**Scope:** The web interface that talks to the Go daemon, focused on the agent chat
experience: failure handling, user input, tool approval, and conversation/model
management. Editor, file tree, and pairing are out of scope except where they
intersect chat.

This spec is written first against the *desired* behaviour, then reconciled with
what is actually implemented (see [Implementation Gap Analysis](#implementation-gap-analysis)).

---

## 1. Vocabulary

| Term | Meaning |
|---|---|
| **Conversation** | A user-visible thread of messages. Stable identity, survives daemon restarts, can be renamed/deleted. Owns the event history. |
| **ACP session** | The transient live connection to an agent process. A conversation has at most one *active* ACP session. Switching model/agent replaces the ACP session but keeps the conversation. |
| **Agent / harness** | A coding agent binary (Codex, Mistral Vibe, Claude Code) reachable over ACP. |
| **Model** | A model offered by an agent. Selected per conversation; changeable mid-conversation. |
| **Event** | An immutable record in the log (`PromptSubmitted`, `StreamUpdate`, `ToolStarted`, …). The chat view is a pure function of the event stream. |

> **Key design decision:** ACP v0.13.5 has no stable per-session model API
> (`NewSessionRequest` has no `ModelId`). Therefore *model/agent switching is a
> client-side concept*: the daemon keeps the conversation + its events and spins
> up a fresh ACP session bound to the newly selected agent/model. The UI must
> reflect that the active model can change without losing history.

---

## 2. Layout ✅

- **Desktop (≥1024px):** activity bar | left sidebar (files/search) | editor | right chat panel (always visible, ~384px).
- **Mobile (<1024px):** one panel at a time via bottom nav (explorer / editor / chat / settings).

Chat panel regions (top → bottom):
1. **Header bar** — agent selector, model selector, connection indicator, conversation menu (hamburger).
2. **Conversation list popout** — toggled from the header; list of conversations with rename/delete/new.
3. **Message list** — rendered from events; scrolls; sticky-to-bottom while streaming.
4. **Status / banner area** — connection loss, errors, pending-permission count.
5. **Composer** — multiline input, send, cancel (while running), attach (future).

---

## 3. Connection & Failure States (the system must handle *every* failure) ✅

The UI MUST surface every failure and never fail silently. Required states:

| State | Trigger | UI behaviour |
|---|---|---|
| **Connecting** | Initial load, WS not yet open | Subtle "Connecting…" pill in header; composer enabled but send queued/blocked with tooltip. |
| **Connected** | WS open | Green dot in header. |
| **Disconnected** | WS closed/error | Amber/red dot + persistent banner "Reconnecting…"; auto-retry with backoff; composer send disabled with reason. |
| **Reconnected** | WS reopens after drop | Banner clears; missed events re-synced via `GET /api/events?after=<lastId>`; toast "Reconnected". |
| **No agents configured** | `agents.length === 0` | Composer disabled, placeholder "Configure an agent in Settings"; inline CTA opening settings. |
| **Agent executable missing** | agent has `warning` | Warning badge on agent in selector + tooltip; sending shows the warning. |
| **Session create failed** | `POST /api/sessions` non-2xx | Inline error bubble in chat with the server message + "Retry"; input text preserved. |
| **Prompt send failed** | `POST …/prompt` non-2xx | Inline error bubble with message + "Retry"; restore the unsent text into composer. |
| **Agent exited / crashed** | `AgentExited` event | Red system bubble "Agent exited: <reason>"; offer "Restart conversation". |
| **Agent error mid-stream** | `AgentExited` with error during run | Same as above; streaming cursor removed. |
| **Permission timeout** | request resolves to deny via ctx timeout | System note "Permission request expired (denied)". |
| **Stale file revision** | write conflict (editor) | Toast "File changed on disk — reload". (editor-owned) |
| **Model/agent switch failed** | switch endpoint error | Toast + keep previous model selected; do not lose history. |
| **Request in flight** | prompt running | Composer shows "Stop" (cancel) instead of send; disable model/agent switch or warn it ends current turn. |

Every network call goes through one error path that extracts `{error}` from the
JSON body and presents a human-readable message.

---

## 4. User Input (Composer) ✅

- Multiline `textarea`, `Enter` sends, `Shift+Enter` newline.
- Send disabled when: empty/whitespace, sending in progress, no agent, no model, disconnected.
- On send:
  1. If no active conversation, create one (lazily) bound to selected agent+model.
  2. Optimistically clear the input; show the user bubble immediately (from `PromptSubmitted` event echoed by backend).
  3. On failure, restore text and show inline error (see table).
- While the agent is running, the send button becomes **Stop** → calls cancel.
- Composer is never wiped on error; text is preserved/restored.

---

## 5. Tool Approval (accept/deny) ✅

Permission prompts arrive as `PermissionRequested` events carrying:
`requestId`, `sessionId`, `tool`, optional `command`/`target`, and `options`
(subset of `allow_once`, `allow_session`, `allow_always`, `deny`).

Requirements:
- Render an inline permission card in the conversation at the point of request.
- Show the tool name and, when present, the command/target/diff being requested.
- Render **one button per `option`** returned by the backend (not a hardcoded
  Allow/Deny). Labels: Allow once / Allow this session / Always allow / Deny.
- Respond via `POST /api/permissions/{requestId}/respond` with the exact
  `decision` string. **Use `requestId`, not `sessionId`.**
- After a response (or if resolved on another device), collapse the card into a
  resolved state ("Allowed once" / "Denied") derived from
  `PermissionGranted` / `PermissionDenied` events.
- Pending permissions must be re-presented after reconnect
  (`GET /api/permissions/pending`).
- Multiple simultaneous prompts are stacked and individually answerable.

---

## 6. Conversation Management ✅

### 6.1 Switching conversations
- Conversation list popout shows all conversations (id, title, status dot, last-activity time).
- Selecting one loads its events (`GET /api/events/{conversationId}`) and makes it active.
- Active conversation is visually highlighted.

### 6.2 Remembering conversations (persistence)
- Conversations persist across daemon restarts (backed by the event store / a sessions table).
- On reload, the UI restores: active conversation, selected agent, selected model, open files. (Use `localStorage` for the active selection + server truth for the list.)
- The conversation list is populated from the server, not just in-memory sessions.

### 6.3 Renaming
- Inline rename (double-click title or "Rename" in a per-row menu).
- Persists via `PATCH /api/sessions/{id}` `{ "name": "…" }`.
- Optimistic update; revert + toast on failure.
- Auto-title: first conversation title defaults to a trimmed version of the first user prompt.

### 6.4 Deleting
- Per-row "Delete" with confirm.
- Calls `DELETE /api/sessions/{id}` (closes ACP session + removes metadata; events may be retained or purged — see backend spec).
- If the deleted conversation was active, clear the view to the empty state.

### 6.5 New conversation
- "New Chat" resets to empty composer state; the backend conversation is created lazily on first send (or eagerly on model/agent pick — see 7).

---

## 7. Model / Agent Switching on the Same Conversation ✅

- Header shows two selectors: **agent (harness)** and **model**.
- Changing the agent repopulates the model list and selects the agent's first model.
- Changing either selector on an **existing** conversation:
  - Persists the new agent/model on the conversation.
  - Replaces the underlying ACP session (old one closed; new one created) while
    **preserving the conversation id and event history**.
  - Inserts a system note in the thread: "Switched to <agent> / <model>".
  - If a turn is currently running, prompt the user that switching will end the
    current turn (cancel), or disable switching until idle.
- The selected agent/model are remembered per conversation and restored on reload.

---

## 8. Message Rendering (event → view) ✅

The chat view renders these event types (no event type may silently disappear):

| Event | Rendering |
|---|---|
| `PromptSubmitted` | User bubble. |
| `ResponseStarted` | "Agent is thinking…" placeholder (replaced by stream). |
| `StreamUpdate` | Agent bubble; consecutive chunks merged; blinking cursor while `streaming`. |
| `AgentThoughtChunk` (new) | Collapsible "Thinking" block, muted text. |
| `ToolStarted` | Tool card with kind icon + title + "running". |
| `ToolCompleted` | Tool card with status, output/diff/locations, collapsible. |
| `Plan` / `PlanUpdate` (new) | Checklist card of plan entries with status. |
| `PermissionRequested` | Permission card (Section 5). |
| `PermissionGranted` / `PermissionDenied` | Resolve the matching permission card. |
| `ShellCommandStarted` / `ShellOutputStreamed` / `ShellCommandCompleted` | Terminal-style card: command, streamed output, exit code. |
| `FileRevisionUpdated` | Subtle "edited <path>" note (optional inline). |
| `SessionCancelled` / `SessionInterrupted` | System note "Stopped". |
| `AgentExited` | Red system bubble with reason. |
| `ConnectionRestarted` / `SessionResumed` | System note. |

Tool cards use the ACP `ToolKind` to choose an icon (read/edit/execute/search/…).

---

## 9. Accessibility & UX details ⏳
- All interactive controls keyboard-reachable; focus rings visible.
- Status colours paired with text/icon (not colour-only).
- Long output scrolls within its card; never breaks layout.
- Errors are dismissible but logged in the thread for history.

---

## Implementation Gap Analysis

> **Status update 2026-06-27:** The "Broken / missing" items below were resolved by Work Streams 1-5 (see `docs/STATUS.md`). The list is retained for history.

Comparison of this spec against the current code (`web/src/...`, `internal/...`).

### Present and roughly correct
- Event-driven chat rendering (`ChatMessageItem.tsx`) for `PromptSubmitted`, `ResponseStarted`, `StreamUpdate`, `ToolStarted`, `ToolCompleted`, `AgentExited`.
- Stream chunk merging (`ChatPanel.mergedEvents`).
- Agent + model selectors with dependent model list.
- Lazy conversation creation on first send.
- Conversation list popout + new chat (`ChatHistory.tsx`).
- Per-call JSON error extraction (`api.ts apiFetch`).
- WebSocket with auto-reconnect (`useBackend.connectWebSocket`).

### Broken / missing (must fix)
1. **Permission prompts never reach the UI.** Backend `PermissionManager.Request()` blocks on a channel but no `PermissionRequested` event is emitted/broadcast. The UI's `PermissionRequested` case is dead code. → backend must emit the event; UI already has a card but wired wrong.
2. **Permission response uses wrong id + invalid decisions.** `onPermissionResponse(event.sessionId, 'allow'|'deny')` sends *sessionId* as the request id and `'allow'`/`'deny'` which are not valid `PermissionDecision`s. → use `requestId` + `allow_once`/`deny`, render one button per `option`.
3. **No rename.** No UI or endpoint (`PATCH /api/sessions/{id}`) to rename conversations.
4. **No delete in UI.** `api.closeSession` exists but is not surfaced; no confirm; `ChatHistory` has no delete affordance.
5. **No mid-conversation model/agent switch.** Selectors only affect *new* sessions; changing them does not re-bind an existing conversation, and there is no backend support.
6. **Conversations not remembered across restart.** Session list is in-memory (`acp.Client.sessions`); names are `Session <id8>`; restart loses them while events linger orphaned.
7. **Connection state not surfaced.** `connected` exists in `useBackend` but is not shown; no banner/toast, no reconnect re-sync of missed events, no pending-permission re-fetch.
8. **No cancel/stop affordance** in the composer while running (`cancelSession` exists in hook but unused in `ChatPanel`).
9. **Thoughts, plans, shell, file-revision events not rendered.** `ChatMessageItem` returns `null` for `AgentThoughtChunk`/`Plan`/shell/file events.
10. **Tool cards lack detail** (kind icon, command/diff/output/locations) — backend drops these fields (`transport.SessionUpdate`).
11. **UI state not persisted** (active conversation, model) — lost on reload.
12. **Error UX is minimal** — single inline red box for the last error; no retry, no per-bubble errors, no agent-missing CTA.

### Spec adjustments made after reviewing implementation
- Kept lazy conversation creation (already implemented and reasonable) rather than eager creation on model pick.
- "Session" in existing code == "Conversation" here; backend will gain a persisted **conversation** record while still using ACP sessions internally. The REST surface keeps `/api/sessions` for compatibility but gains `PATCH` (rename) and richer `GET` (name, agentId, modelId, status, updatedAt).
- Permission card renders dynamic options from the backend instead of a fixed Allow/Deny pair.
- Model switching is explicitly a client-side re-bind (no ACP model API in v0.13.5).
