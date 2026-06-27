# Chat Panel Feature Spec

> **Status update 2026-06-27:** EXISTS = shipped in current codebase; NEW = backlog. See `docs/plans/OpenItems.md` section 5 for UI feature requests.
> Companion to `mockup-chat-panel.html`. Covers every ACP event/feature in `docs/acp/responsibilities.md`.
> Implementation status: **EXISTS** = in `ChatPanel.tsx`/`ChatMessageItem.tsx` today; **NEW** = desired state from mockup.

## Composer Bar

- **Text input** (`<textarea>`) — multi-line, Enter sends, Shift+Enter inserts newline. **EXISTS**
  - Disabled while `sending` or `agents.length === 0`. Placeholder: "Message agent..." (or "Configure an agent first..." if no agents).
  - On send failure, input is restored with the unsent text so the user can retry.
- **Send button** (arrow-up icon, blue) — bottom-right of input. **EXISTS**
  - Disabled when input empty, no agent/model selected, or `sending`. Clicking calls `onSendMessage(sessionId, content)`.
  - If no active session, `onCreateSession(agentId, modelId)` is called first, then the message is sent to the new session.
- **Stop/Cancel button** (square icon, red) — replaces Send while a turn is in flight. **EXISTS**
  - Visible when `agentRunning` (last event is `ResponseStarted`/`ToolStarted`/`ShellCommandStarted`/streaming `StreamUpdate`, or `sending`).
  - Clicking calls `onCancel(sessionId)` → sends `session/cancel` to the agent.
- **Model selector dropdown** — lists models for the current agent. **EXISTS**
  - Disabled while `sending` or no agent selected. Changing it calls `onRebindSession(sessionId, agentId, modelId)`.
- **Harness selector dropdown** — lists agents (Claude Code, Codex, Gemini CLI, custom). **EXISTS**
  - Disabled while `sending`. Changing it resets the model to the agent's first model and rebinds the active session.
- **Harness lock toggle** (lock/unlock icon button). **NEW**
  - When locked: harness selector is disabled, lock icon shows closed/active state. Pins the chat to the current harness so accidental selector changes don't rebind mid-conversation.
  - When unlocked: selector behaves normally. Lock state is per-conversation (persisted with session metadata).
- **Attachment button** (paperclip) — placeholder for future image/artifact upload. **EXISTS** (stub; image upload is out of scope per OpenItems).

## Chat Message Area

Every event from the ACP event stream is rendered chronologically. Consecutive `StreamUpdate` events of the same role/thought-flag are merged into one growing message.

- **User message** (`PromptSubmitted`) — right-aligned bubble, blue-tinted, user avatar "U". **EXISTS**
- **Agent streaming text** (`StreamUpdate` role=agent, `agent_message_chunk`) — left-aligned, agent avatar, text grows as chunks arrive. A blinking cursor appends while `streaming=true`. **EXISTS**
- **Agent thought** (`StreamUpdate` thought=true, `agent_thought_chunk`) — collapsed `<details>` with italic "Thinking…" summary; body is muted text behind a left border. **EXISTS**
- **Tool call cards** (`tool_call` / `tool_call_update`) — collapsible `<details>` with chevron, kind icon, tool name, target, and status. **EXISTS**
  - **read** (file-search icon, blue) — shows file path; expanded body shows file content.
  - **edit** (file-pen icon, amber) — shows file path; expanded body shows a **diff block** with red/green lines. **Diff rendering is NEW** (current code shows raw content).
  - **execute** (play icon, green) — shows command; expanded body shows command text.
  - **search** (file-search icon, blue) — shows query; expanded body shows match summary.
  - Status states: `[running]` (gray, `ToolStarted`), `[completed]` (gray, `ToolCompleted`), `[failed]` (red, `ToolCompleted` with summary=failed).
- **Plan checklist** (`plan` update) — card with checklist items. **EXISTS as plain-text render; NEW: status-coded checklist**
  - Each entry has a status: **pending** (empty circle), **in_progress** (blue pulsing dot), **completed** (green check, strikethrough text).
- **Shell command card** (`terminal/create` + `terminal/output` + `terminal/wait_for_exit`) — black terminal block. **EXISTS**
  - Header: `$ <command>` + exit code (green for 0, red for non-zero). Body: command output (`terminal/output` streamed).
- **Permission prompt card** (`session/request_permission`) — blue-tinted card with shield icon. **EXISTS**
  - Shows tool kind + target + command (if any). Four option buttons (see Permission Flow).
  - When resolved: card collapses to a muted one-line summary ("Permission granted/denied — <tool>").
- **File revision note** (`FileRevisionUpdated`) — centered pill: "edited config.json — revision 3". **EXISTS**
- **Agent exited/error** (`AgentExited`) — red-tinted agent avatar, error message in red text. **EXISTS**
- **Session events** (centered pills): `SessionCancelled`/`SessionInterrupted` → "Stopped"; `ConnectionRestarted`/`SessionResumed` → "Session restarted" with refresh icon. **EXISTS**
- **Error banner** — red-tinted block at bottom of messages area when a send fails. **EXISTS**
- **Turn end / stop reason** (`session/prompt` response) — when the agent ends its turn, the streaming cursor disappears and the Stop button reverts to Send. No explicit UI element; the state transition from running→idle is the signal. **EXISTS**

## Conversation Header

- **Inline-renameable conversation name** — text input that looks like a label; click to edit, Enter/blur to save via `onRenameSession`. **NEW** (current header has harness/model only; name editing is in ChatHistory popout).
- **Agent + model badge** — pill showing agent name + model (e.g., "Claude Code · sonnet-4"). **NEW** (currently only selectors show this).
- **Export button** (download icon) — calls `onExportSession(sessionId)`, downloads conversation transcript. **EXISTS** (in ChatHistory popout; NEW to header).
- **Delete button** (trash icon, red hover) — calls `onDeleteSession(sessionId)`. **EXISTS** (in ChatHistory popout; NEW to header).
- **Rebind indicator** (amber badge with refresh icon) — shown when the conversation has been rebound to a different agent than it started with (e.g., "Rebound from Codex"). **NEW**
- **Connection indicator** (wifi/wifi-off icon) — green when connected, red when disconnected. **EXISTS**
- **Chat history menu** (hamburger icon) — toggles the ChatHistory popout listing all sessions. **EXISTS**

## Harness & Model Switching

- The active conversation **owns** its agent/model. Selectors derive from the active session: `effectiveAgentId = activeSession.agentId || selectedAgent`.
- For a **new chat** (no active session), local selector state drives the choice; the session is created on first send.
- **Switching harness** on an active session calls `onRebindSession(sessionId, newAgentId, firstModelId)`:
  - Daemon closes the old agent session (graceful `session/delete` if supported — Stream 1) and starts a new one with the new agent.
  - **Context is NOT auto-migrated** — the new agent starts fresh. The conversation transcript (event history) remains visible in the UI so the user can reference it.
  - A **rebind indicator** appears in the header noting the original agent.
- **Switching model** on an active session calls `onRebindSession(sessionId, sameAgentId, newModelId)`:
  - Same agent process, new model. Context is preserved (same ACP session if the agent supports it; otherwise a new session is created transparently).
- **Harness lock** — when engaged, the harness selector is disabled and the lock icon shows closed. This prevents accidental rebinds. The model selector remains enabled (model switches don't change harness). Unlocking restores selector function. Lock state is per-conversation.
- Both selectors are **disabled while `sending`** to prevent rebind mid-turn.

## Permission Flow

- **Appearance**: when the agent sends `session/request_permission`, a `PermissionRequested` event arrives and renders a blue-tinted permission card inline in the message stream (at the point in the conversation where the request occurred).
- **Card content**: shield-alert icon + "Permission Required" header; tool kind (e.g., `execute`, `edit`); target path or command; the command text in a mono block if present.
- **Options shown** (4 buttons in a 2×2 grid, per ACP spec):
  - **Allow once** (blue) — `allow_once`: permits this single operation; future requests of the same kind+target will prompt again.
  - **Allow always** (blue) — `allow_always`: permits this and all future same-(session, toolKind, target) requests without prompting (Stream 2 policy enforcement).
  - **Reject once** (gray) — `reject_once`: denies this single operation; agent may request again.
  - **Reject always** (gray) — `reject_always`: denies this and all future same-kind requests in this session.
- **Response dispatch**: clicking a button calls `onPermissionResponse(requestId, decision)`. The daemon sends the `session/request_permission` response to the agent. Buttons disable immediately after click.
- **Multi-device**: any paired device can respond. If another device responds first, the card collapses to the resolved state on this device.
- **Resolved appearance**: the card transitions to a muted state — buttons hidden, a one-line summary shows the outcome ("Permission granted — execute" or "Permission denied — execute"). The tool kind is struck through for denials.
- **Auto-resolved (policy)**: when `allow_always`/`allow_session` policy matches, the daemon auto-resolves without UI prompt; the card still appears in the stream but in pre-resolved state with a note that it was auto-approved. **NEW** (Stream 2).
- **Stale requests**: if the pending permission is no longer active (e.g., session closed), buttons disable and a note "This request is no longer active." appears. **EXISTS**.

## Connection State

- **Connected** (default): no banner. Connection indicator in header shows green wifi icon.
- **Reconnecting**: amber banner below header — "Reconnecting to daemon…" with wifi-off icon. Selectors and input disable. **EXISTS**.
- **Disconnected**: red banner — "Disconnected from daemon". All controls disable. Messages remain visible (read-only).
- **Reconnect flow**: the WebSocket client auto-reconnects with backoff. On success, the banner clears, the event stream re-syncs (daemon replays missed events from the SQLite store), and controls re-enable.
- **In-flight prompt handling**: if a prompt was mid-flight when connection dropped, a pill appears: "Prompt in flight — will resume on reconnect". On reconnect, the daemon checks if the agent session is still alive; if so, streaming continues; if not, a `SessionResumed`/`ConnectionRestarted` pill appears and the user can re-send. **NEW** (pills exist; in-flight surfacing is new).
- **Session resumed**: on reconnect, if the ACP session was preserved (`session/load` — Stream 1), a "Session resumed — context preserved" pill appears. If the session was lost, the conversation rebinds to a fresh session and a rebind indicator appears.

## Mobile

- **Bottom-nav layout**: on screens < 1024px, the chat panel is full-screen when the "chat" nav item is active. Only one panel is visible at a time (Explorer, Editor, Chat, Settings).
- **Panel switching**: the chat panel uses `absolute inset-0 z-30` on mobile (vs. `lg:relative` on desktop) so it overlays the editor when active.
- **Composer**: the composer has extra bottom padding (`pb-20`) on mobile to clear the bottom nav bar; `lg:pb-3` on desktop.
- **Messages**: padding is `p-3` on mobile, `lg:p-4` on desktop; spacing is `space-y-3` mobile, `lg:space-y-4` desktop.
- **Header**: all header controls (harness, model, lock, name, badges, actions) wrap or scroll horizontally on narrow screens. The conversation name input flexes to fill available space.
- **Tool cards / diffs**: diff blocks and tool cards scroll horizontally (`overflow-x-auto`) on narrow screens rather than wrapping.
- **Permission cards**: the 2×2 option grid stays 2×2 on mobile (buttons are small enough); card padding reduces.
