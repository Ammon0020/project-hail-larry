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
- **Session management** — `session/new`, `session/cancel`, `session/list` (our layer); ⚠️ _Planned:_ `session/load` (ACP `LoadSession`), `session/delete` (ACP `DeleteSession`) — currently sessions are recreated fresh on restart and `CloseSession` kills the process without calling ACP delete
- **Prompt dispatch** — sends `session/prompt` when the user submits a message
- **Update forwarding** — handles all `session/update` notifications, forwards to web UI via WebSocket
- **File reads** — implements `ReadTextFile` against the workspace filesystem
- **File writes** — implements `WriteTextFile` against the workspace filesystem
- **Terminal execution** — implements `CreateTerminal`, `TerminalOutput`, `WaitForTerminalExit`, `KillTerminal`, `ReleaseTerminal` via `internal/shell`
- **Permission UI** — implements `RequestPermission` by emitting a prompt event to the web UI, waiting for user decision from any paired device, responding to the agent
- **Permission policy** — ⚠️ _Planned:_ track `allow_always` / `reject_always` to auto-resolve future requests; currently every permission request blocks for user input (audit log records decisions but no policy enforcement)

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
