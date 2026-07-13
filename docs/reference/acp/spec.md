# ACP Specification Reference

ACP standardizes communication between code editors/IDEs and AI coding agents (like LSP for language servers). The Client (our daemon) owns the filesystem, shell, permissions, and session state; the Agent (Claude Code, Codex, Gemini CLI) owns the LLM reasoning loop. Agents never touch the filesystem directly — they request operations via JSON-RPC and the client executes them.

## 1. Roles & Responsibilities

- **Client (context provider/owner):** manages and sends context to the agent via session/prompt using ContentBlock elements; owns filesystem + editor state; implements `fs/read_text_file`, `fs/write_text_file`, `terminal/*`, `session/request_permission`; manages session lifecycle (`new`/`load`/`cancel`/`delete`); spawns and manages the agent subprocess via stdio.
- **Agent (context consumer):** receives context from the client; runs the LLM reasoning loop; reports progress via `session/update` notifications (`agent_message_chunk`, `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`); requests files via `fs/read_text_file`; requests terminals; requests permissions; ends each turn with a `stopReason`.
- **Proxy pattern (optional):** a middleware layer sitting between client and agent can inject context, provide contextual data for tools, and customize system prompts. The flow is Client → (optional Proxies) → Agent, with the client remaining the primary context authority.

## 2. Initialize Handshake

The client initiates the protocol by sending an `initialize` request over JSON-RPC:

```json
{
  "jsonrpc": "2.0",
  "id": 0,
  "method": "initialize",
  "params": {
    "protocolVersion": 1,
    "clientCapabilities": {
      "fs": { "readTextFile": true, "writeTextFile": true },
      "terminal": true
    },
    "clientInfo": {
      "name": "my-client",
      "title": "My Client",
      "version": "1.0.0"
    }
  }
}
```

Notes:

- The v2 draft uses `protocolVersion` `"2025-11-25"` and adds an `elicitation` capability with `form` and `url` sub-objects.
- Agents MUST treat any omitted capability as `false`.
- The agent responds with its `AgentCapabilities`, which may include: `session`, `auth`, `loadSession`, `nes`, `positionEncoding`, `providers`, and `promptCapabilities` (including image support).

## 3. Session Lifecycle

- **`session/new`** — the client creates a new session, optionally passing `cwd` and `mcpServers`.
- **`session/load`** — resume a persisted session by ID. Only valid when the agent advertised the `loadSession` capability during initialize.
- **`session/prompt`** — send a user message to the agent as a `ContentBlock[]`. Example with text + resource blocks:

```json
{
  "jsonrpc": "2.0",
  "id": 2,
  "method": "session/prompt",
  "params": {
    "sessionId": "sess_abc123def456",
    "prompt": [
      { "type": "text", "text": "Can you analyze this code for potential issues?" },
      {
        "type": "resource",
        "resource": {
          "uri": "file:///home/user/project/main.py",
          "mimeType": "text/x-python",
          "text": "def process_data(items):\n    for item in items:\n        print(item)"
        }
      }
    ]
  }
}
```

- **`session/cancel`** — notification to abort in-progress work. The client MUST respond to any outstanding permission requests with the `"cancelled"` outcome.
- **`session/delete`** — unstable method to delete a session.
- **`session/list`** — unstable/optional method to list the agent's known sessions. The client can use this to reconcile its session map.
- The agent responds to `session/prompt` with a result containing a `stopReason` (e.g. `end_turn`).

## 4. session/update Notifications

The agent streams progress to the client via `session/update` notifications. Each notification carries one of the following update types:

- **`agent_message_chunk`** — streamed assistant text.
- **`agent_thought_chunk`** — streamed reasoning/thought text.
- **`tool_call`** — a new tool call has started. Fields include `kind`, `status`, `title`, `locations`, and `rawInput`.
- **`tool_call_update`** — progress or result for a tool call. Fields: `toolCallId`, `status`, and a `content` array containing `text`, `diff`, or `terminal` blocks.
- **`plan`** — execution plan entries with `content`, `priority`, and `status`.
- **`user_message_chunk`** — echoed user message text.

## 5. Filesystem Methods

- **`fs/read_text_file`** — the agent requests a file's contents; the client reads from the workspace filesystem and returns the content. The agent MUST verify the client advertised the `fs.readTextFile` capability before calling.
- **`fs/write_text_file`** — the agent requests a file write; the client writes the content to the workspace filesystem. The agent MUST verify the `fs.writeTextFile` capability before calling.

## 6. Terminal Methods

- **`terminal/create`** — params:
  - `command` (required)
  - `args` (optional)
  - `cwd` (optional, absolute path)
  - `env` (optional, `EnvVariable[]`)
  - `outputByteLimit` (optional)
  - `sessionId` (required)

  Returns a `terminalId`.

- **`terminal/output`** — returns buffered output, a truncation flag, and exit status.
- **`terminal/wait_for_exit`** — blocks until the process exits; returns `exitCode` and `signal` (either may be `null`).
- **`terminal/kill`** — kills the process but keeps the `terminalId` valid for further queries.
- **`terminal/release`** — kills the process and removes the terminal entry. The agent is responsible for releasing terminals it no longer needs.

## 7. Permissions

- **`session/request_permission`** — the agent sends the `toolCall` details plus an `options` array. Each option has: `optionId`, `name`, and `kind`. Standard kinds are:
  - `allow_once`
  - `allow_always`
  - `reject_once`
  - `reject_always`

  Custom kinds MUST begin with an underscore (`_`).

- The client responds with an outcome of either:
  - `selected` — includes the chosen `optionId`.
  - `cancelled` — returned when the active work was cancelled.

- Clients MAY auto-allow or auto-reject permission requests according to user settings.

## 8. Content Blocks

The `prompt` field of `session/prompt` is a `ContentBlock[]`. Supported block types:

- **`text`** — plain text.
- **`resource`** — a resource with `uri`, `mimeType`, and `text` (inline content). This is the primary mechanism by which the client sends file context to the agent.
- **`resource_link`** — a reference to a resource by URI, with no inline content.
- **`image`** — an inline image with `data`, `mimeType`, and optional `uri`.

## 9. Context Responsibility (Key Principle)

The CLIENT is the context authority. It sends relevant files and context directly in the `session/prompt` request as `resource` ContentBlocks. If the agent needs additional context, it requests it via `fs/read_text_file`. The agent does not maintain its own system prompts or folder state. Proxies may inject additional context between the client and the agent, but the client remains the authoritative source of context.

---

Verified against the official ACP spec via Context7 (`/agentclientprotocol/agent-client-protocol`). For up-to-date details, query Context7 or visit agentclientprotocol.com.
