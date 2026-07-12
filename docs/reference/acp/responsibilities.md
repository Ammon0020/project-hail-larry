# ACP: Division of Responsibilities

Verified against the official ACP spec via [agentclientprotocol.com](https://agentclientprotocol.com) and Context7 (`/coder/acp-go-sdk`).

> **When working on ACP-related code, use Context7 to fetch up-to-date docs.**
> Resolve the library with `mcp0_resolve-library-id("acp go sdk coder")`, then call `mcp0_get-library-docs` with the returned ID. The official site is at [agentclientprotocol.com](https://agentclientprotocol.com) — key pages: [/protocol/overview](https://agentclientprotocol.com/protocol/overview), [/protocol/v1/file-system](https://agentclientprotocol.com/protocol/v1/file-system), [/protocol/v1/terminals](https://agentclientprotocol.com/protocol/v1/terminals), [/protocol/v1/tool-calls](https://agentclientprotocol.com/protocol/v1/tool-calls).

## What the Agent Handles (e.g., Claude Code)

- **LLM reasoning loop** — prompts the model, decides actions, maintains autonomous reasoning
- **Tool call reporting** — sends `session/update` with `tool_call` / `tool_call_update` (kind, status, title, content, locations)
- **Permission requests** — sends `session/request_permission` with options (`allow_once`, `allow_always`, `reject_once`, `reject_always`)
- **File read/write requests** — sends `fs/read_text_file` and `fs/write_text_file` JSON-RPC requests to the client
- **Terminal requests** — sends `terminal/create`, `terminal/output`, `terminal/wait_for_exit`, `terminal/kill`, `terminal/release`
- **Message/thought streaming** — sends `session/update` with `agent_message_chunk` and `agent_thought_chunk`
- **Plan updates** — sends `session/update` with `plan`
- **Stop reason** — sends the final `session/prompt` response when the turn ends

## What Our Daemon Handles (ACP Client)

- **Agent subprocess lifecycle** — spawns the agent via stdio, manages the process
- **Initialize handshake** — advertises `ClientCapabilities` (`fs.readTextFile`, `fs.writeTextFile`, `terminal: true`)
- **Session management** — `session/new`, `session/cancel`, `session/list` (our layer); ✅ `session/load` (ACP `LoadSession`) attempted on restart when the agent advertises `loadSession` and a persisted `acpSessionId` exists — falls back to `session/new` on any failure (capability unsupported, session gone); ✅ `session/delete` (ACP `UnstableDeleteSession`, best-effort) called in `CloseSession` before killing the process and via `CloseAllSessions` on daemon shutdown. `ACPSessionID` is persisted in `conversations.json` so resume works across restarts.
- **Prompt dispatch** — sends `session/prompt` when the user submits a message
- **Update forwarding** — handles all `session/update` notifications, forwards to web UI via WebSocket
- **File reads** — implements `ReadTextFile` against the workspace filesystem
- **File writes** — implements `WriteTextFile` against the workspace filesystem
- **Terminal execution** — implements `CreateTerminal`, `TerminalOutput`, `WaitForTerminalExit`, `KillTerminal`, `ReleaseTerminal` via `internal/shell`
- **Permission UI** — implements `RequestPermission` by emitting a prompt event to the web UI, waiting for user decision from any paired device, responding to the agent
- **Permission policy** — ✅ Implemented: `allow_always` / `allow_session` / `reject_always` decisions are cached in session-scoped policy maps keyed by `(sessionID, tool, target)`. Allow decisions auto-resolve subsequent identical requests without re-prompting; `reject_always` auto-denies without re-prompting. `allow_once` / `reject_once` still prompt every time. Policies are dropped via `ClearSession` when a session closes.
- **Context injection** — ✅ Context (workspace info, file tree, git status, AGENTS.md, open file contents, editor selection) is sent as structured `resource` ContentBlocks when the agent advertises `embeddedContext`, with a `resource_link` + text fallback for agents that don't. Open files and selection are sent with every prompt; workspace context on the first prompt only.
- **Stop reason** — ✅ The agent's `stopReason` from the `session/prompt` response is captured and forwarded to the frontend in the final `StreamUpdate` event. The UI displays non-normal stop reasons (e.g. "hit token limit", "refused").
- **Protocol version** — ✅ Pinned to `acp.ProtocolVersionNumber` (v1) in the `InitializeRequest`.
- **Terminal env** — ✅ Agent-supplied environment variables (`terminal/create` `env` param) are overlaid on the daemon environment and passed to the subprocess.
- **Terminal signal** — ✅ Signal termination is detected via `syscall.WaitStatus` and the signal name (e.g. "killed", "terminated") is populated in `TerminalExitStatus.Signal`.

## Use Case: User Requests a File Write

User types *"Create a config.json with debug mode enabled"* in the web UI.

| Step | Actor | Action | Protocol Method |
|------|-------|--------|-----------------|
| 1 | User | Types message in web UI | — |
| 2 | Daemon | Sends prompt to agent over stdio | `session/prompt` |
| 3 | Agent | LLM processes request, decides to write `config.json` | (internal) |
| 4 | Agent | Streams thought: *"I'll create config.json"* | `session/update` → `agent_message_chunk` |
| 5 | Agent | Reports tool call: kind `edit`, status `pending` | `session/update` → `tool_call` |
| 6 | Agent | Requests permission to proceed | `session/request_permission` |
| 7 | Daemon | Emits permission prompt event to web UI via WebSocket | (internal) |
| 8 | User | Clicks *"Allow once"* on paired device | — |
| 9 | Daemon | Sends permission response to agent | `session/request_permission` response |
| 10 | Agent | Sends file write request with path + content | `fs/write_text_file` |
| 11 | Daemon | Writes file to workspace filesystem | (internal) |
| 12 | Daemon | Sends success response to agent | `fs/write_text_file` response |
| 13 | Agent | Updates tool call to `completed`, includes diff | `session/update` → `tool_call_update` |
| 14 | Agent | Streams summary message | `session/update` → `agent_message_chunk` |
| 15 | Agent | Ends turn with stop reason | `session/prompt` response |
| 16 | Daemon | Forwards all updates to web UI — file appears in editor with diff indicators, summary in chat | (WebSocket) |

**Key principle:** The agent never touches the filesystem directly. It *requests* operations via JSON-RPC; the daemon *executes* them. The permission gate sits between intent and execution, giving the user veto power from any paired device.

## SDK Features Not Yet Adopted

Available in `coder/acp-go-sdk v0.13.5` (Go) and/or `@agentclientprotocol/sdk` (TypeScript) but not wired into the daemon. See `docs/plans/acp-spec-compliance.md` Priority 4 for adoption plan and rationale.

| Feature | SDK Support | Priority | Notes |
|---------|------------|----------|-------|
| Audio blocks | `acp.AudioBlock()` | Future | No use case |
| Session list | `Agent.ListSessions()` (stable v0.11.7) | ✅ Done | Reconcile after restart (P4.4) |
| Elicitation | Unstable methods | Future | v2 draft — high interest once stable |
| NES | Unstable methods | Future | Inline completions |
| MCP-over-ACP | Unstable methods | ⛔ Blocked (SDK) | `mcp/connect`+`mcp/disconnect` wired in v0.13.5 but **`mcp/message` relay is not code-generated** and can't be wired via the stock `ClientSideConnection`. Inline transport retained. Blocker + drop-in design: `docs/plans/acp-spec-compliance.md` §4.10 |
| Provider mgmt | Unstable methods | ✅ Done | Session-scoped `providers/list\|set\|disable`; REST `/api/sessions/{id}/providers`; Settings → General UI (P4.11) |
| Tool content helpers | `ToolContent`, `ToolDiffContent`, `ToolTerminalRef` | ✅ N/A | Agent-side only; we are the client |
| Explicit validation | `Validate()` on requests | ✅ Done | All constructed requests (P4.2) |
| AdditionalDirectories | v0.13.5 | ✅ Done | Multi-root via registered workspaces (P4.5) |
| Session fork/resume/close | `SessionCapabilities` | Future | Agent-advertised |
| TypeScript SDK | `@agentclientprotocol/sdk` | ✅ Scoped | Local `StopReason` union only; UI uses app view-models |
| ACP-inspector | Community tool | Future | Protocol validation testing |
