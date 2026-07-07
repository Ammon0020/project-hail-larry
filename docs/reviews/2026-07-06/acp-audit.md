# ACP Implementation Audit

This audit compares the current ACP client implementation in `internal/acp/` against the official ACP specification (see `spec.md`). Findings are categorized as **Correct**, **Deviation**, or **Gap**.

The implementation is spread across four primary files:

- `internal/acp/acp.go` — `Client`, session lifecycle, prompt pipeline.
- `internal/acp/transport.go` — `Transport`, `Initialize`, `Prompt`, and the `acpClientImpl` with `SessionUpdate`, `RequestPermission`, `ReadTextFile`, `WriteTextFile`.
- `internal/acp/terminal.go` — terminal methods.
- `internal/acp/context.go` — prompt middleware pipeline (`FirstPromptContextMiddleware`).

## Correctly Implemented (matches spec)

1. **Initialize handshake** — `transport.go` `Initialize()` advertises `ClientCapabilities{Fs:{ReadTextFile:true, WriteTextFile:true}, Terminal:true}` and sends `ClientInfo{name, version}`. Matches spec. (`internal/acp/transport.go:457-474`)

2. **Filesystem methods** — `ReadTextFile` and `WriteTextFile` are implemented against the workspace manager. Paths are converted to workspace-relative and validated. The agent never touches the filesystem directly. Matches spec. (`internal/acp/transport.go:347-389`)

3. **Terminal methods** — `CreateTerminal`, `TerminalOutput`, `WaitForTerminalExit`, `KillTerminal`, `ReleaseTerminal` are all implemented. Cwd is validated against the workspace root (prevents escape). Output is capped with front-truncation on UTF-8 boundaries. Matches spec. (`internal/acp/terminal.go`)

4. **session/update handling** — `SessionUpdate` switches on all update types: `agent_message_chunk`, `agent_thought_chunk`, `tool_call`, `tool_call_update`, `plan`, `user_message_chunk`. Each is translated to the internal Event system and forwarded to the UI. Matches spec. (`internal/acp/transport.go:34-109`)

5. **session/request_permission** — echoes the agent's own option set back (optionId, name, kind). Returns `selected` with the chosen optionId, or `cancelled` outcome when the context is cancelled or timed out. Matches spec. (`internal/acp/transport.go:157-242`)

6. **Session lifecycle** — `session/new`, `session/load` (capability-gated via `initResp.AgentCapabilities.LoadSession`, falls back to new on failure), `session/delete` (best-effort unstable method), `session/cancel` (notification, keeps the process alive). `ACPSessionID` is persisted for resume across restarts. Matches spec. (`internal/acp/acp.go:275-350`, `transport.go:478-519`)

7. **Permission policy caching** — `allow_always`/`allow_session` decisions are cached per `(sessionID, tool, target)` and auto-resolve subsequent identical requests. Matches the spec's "Clients MAY automatically allow or reject permission requests according to user settings."

8. **Context injection (proxy pattern)** — `FirstPromptContextMiddleware` injects workspace context (path, OS, file tree, git status, `AGENTS.md`) into the first prompt. This aligns with the spec's proxy/middleware pattern and the "client is the context authority" principle. (`internal/acp/context.go`)

9. **Image attachments** — `Prompt()` sends inline `ImageBlock`s when the agent advertises `promptCaps.Image`, otherwise falls back to `ResourceLinkBlock` + text hint. Capability-gated, matches spec intent. (`internal/acp/transport.go:528-561`)

## Deviations (works but not idiomatic ACP)

1. **Context injected as flattened text, not structured resource ContentBlocks.** The spec shows context/files sent as `resource` ContentBlocks in the `session/prompt` array (with `uri`, `mimeType`, `text`). The current implementation prepends a markdown text bundle to the user's prompt string and sends it as a single `TextBlock`. This works but loses structure — the agent cannot distinguish injected context from user text, and cannot use the resource URIs for reference. Location: `internal/acp/acp.go:387-399` (pipeline prepends `injected + "\n\n---\n\n" + content`) and `transport.go:530` (`acp.TextBlock(content)`).

2. **Stop reason discarded.** The spec says the agent responds to `session/prompt` with a `stopReason` (e.g. `end_turn`, `tool_use`, etc.). The current code calls `t.conn.Prompt()` and discards the result (`_, err := t.conn.Prompt(...)`) — it emits its own generic "completed" event instead. The agent's stop reason is never surfaced to the UI. Location: `internal/acp/transport.go:556-560` and `acp.go:456-468`.

3. **Open files / editor selection not sent as context.** The spec's intent is that the client (which owns editor state) sends open files and selected code as resource blocks. The implementation only injects a file-tree listing as text on the first prompt; it does not send the currently-open file or selected code as a resource ContentBlock with each prompt.

## Gaps (missing or incomplete)

1. **`reject_always` auto-deny not implemented.** The permission policy caches `allow_always`/`allow_session` but there is no `reject_always` constant or auto-deny logic. Documented as a known gap in `docs/reference/acp/responsibilities.md`. Location: permission policy in `internal/permissions/`.

2. **Terminal `env` parameter ignored.** The spec's `terminal/create` accepts an `env` (`EnvVariable[]`) parameter. `CreateTerminal` does not pass environment variables to the executor — the subprocess inherits the daemon's environment. Location: `internal/acp/terminal.go:112-192`.

3. **Terminal `signal` not captured.** `terminal/wait_for_exit` returns both `exitCode` and `signal`. The implementation sets `ExitCode` but never populates `Signal` (processes killed by signal report a nil exit code with no signal surfaced). Location: `internal/acp/terminal.go:173-175`.

4. **MCP servers not provisioned.** `NewSession` and `LoadSession` pass `McpServers: []acp.McpServer{}` (empty). The spec allows the client to provision MCP servers to the agent. Not a bug, but a feature gap if MCP tool exposure is desired. Location: `transport.go:479-482`, `496-500`.

5. **Protocol version not explicitly set.** `Initialize()` does not set `protocolVersion` explicitly — it relies on the SDK default. The spec uses version `1` (v1) or `"2025-11-25"` (v2 draft). Worth pinning to a known version. Location: `transport.go:458-473`.

6. **`elicitation` capability not advertised.** The v2 draft supports `elicitation: {form:{}, url:{}}` in client capabilities. Not advertised. Optional/newer feature. Location: `transport.go:466-472`.

7. **No `session/list` ACP call.** The client maintains its own session map but does not call the agent's `session/list` (if supported) to reconcile. Sessions are tracked client-side only. Minor — acceptable for the current architecture.

## Summary Table

| Area | Status | Notes |
| --- | --- | --- |
| Initialize | Correct | Capabilities and client info advertised; protocol version not pinned (see Gap 5). |
| fs/read_text_file | Correct | Workspace-relative, validated. |
| fs/write_text_file | Correct | Workspace-relative, validated. |
| terminal/create | Deviation | `env` parameter ignored (see Gap 2). |
| terminal/output | Correct | UTF-8-safe front-truncation. |
| terminal/wait_for_exit | Deviation | `signal` not captured (see Gap 3). |
| terminal/kill | Correct | Implemented. |
| terminal/release | Correct | Implemented. |
| session/new | Correct | Capability-gated, ID persisted. |
| session/load | Correct | Falls back to new on failure. |
| session/prompt (content blocks) | Deviation | Context sent as flattened `TextBlock` rather than structured `resource` ContentBlocks (see Deviation 1, 3). |
| session/cancel | Correct | Notification, keeps process alive. |
| session/delete | Correct | Best-effort unstable method. |
| session/update handling | Correct | All update types translated to internal events. |
| session/request_permission | Correct | Option set echoed; `cancelled` on timeout. |
| permission policy | Gap | `allow_always`/`allow_session` cached; `reject_always` missing (see Gap 1). |
| context injection | Deviation | Proxy pattern implemented but as text, not resource blocks (see Deviation 1). |
| stop reason | Deviation | Agent `stopReason` discarded (see Deviation 2). |
| MCP provisioning | Gap | Empty `McpServers` slice (see Gap 4). |

---

See `docs/reference/acp/spec.md` for the authoritative spec and `docs/plans/acp-spec-compliance.md` for the planned changes.
