# ACP Spec Compliance Plan

This plan addresses the deviations and gaps identified in the ACP audit (`docs/reviews/2026-07-06/acp-audit.md`), ordered by impact. Each item includes the rationale, the suggested change, and the affected files.

## Priority 1: High Impact

### 1.1 Send context as structured resource ContentBlocks instead of flattened text

**Rationale:** The spec sends file context as `resource` ContentBlocks (uri, mimeType, text) in the `session/prompt` array. The current implementation flattens workspace context into a markdown text string prepended to the user prompt. This loses structure — the agent can't distinguish context from user input or reference files by URI.

**Suggested change:** Modify `Transport.Prompt()` (`internal/acp/transport.go:528-561`) to accept structured context items and build `acp.ContentBlock` resource blocks alongside the text block. Update the pipeline (`internal/acp/context.go`) to return a list of resource blocks (file path, mimeType, content) rather than a single markdown string. Keep the text-block fallback for agents that don't advertise resource handling.

**Affected files:** `internal/acp/transport.go`, `internal/acp/context.go`, `internal/acp/acp.go` (SendPrompt pipeline integration).

### 1.2 Surface the agent's stop reason

**Rationale:** The agent responds to `session/prompt` with a `stopReason` (end_turn, tool_use, max_tokens, etc.). The current code discards it (`_, err := t.conn.Prompt(...)`). The UI can't distinguish "turn ended naturally" from "hit token limit" from "stopped for tool use."

**Suggested change:** Capture the `PromptResponse` result from `t.conn.Prompt()` in `transport.go:556`. Return the stop reason to the caller. In `acp.go` SendPrompt goroutine (`acp.go:456-468`), include the stop reason in the final StreamUpdate event (add a `StopReason` field to the Event struct) so the frontend can display it.

**Affected files:** `internal/acp/transport.go` (Prompt signature + return), `internal/acp/acp.go` (SendPrompt), `internal/interfaces/` (Event struct), frontend chat components.

### 1.3 Send open files / editor selection as resource blocks with each prompt

**Rationale:** The client owns editor state. The spec's intent is that relevant open files and selected code are sent as resource ContentBlocks so the agent has immediate context. Currently only a file-tree text listing is injected on the first prompt.

**Suggested change:** Add an "open files" context provider that reads the editor's open file list and current selection from the frontend (via the existing WebSocket state or a new REST endpoint). Include these as resource blocks in every prompt, not just the first. Cap by size to avoid blowing the context window.

**Affected files:** `internal/acp/context.go` (new middleware), `internal/acp/transport.go` (Prompt), frontend (send open-file state), `internal/server/` (endpoint to query open files).

## Priority 2: Medium Impact

### 2.1 Implement `reject_always` auto-deny

**Rationale:** The permission policy caches `allow_always`/`allow_session` but not `reject_always`. A user who picks "reject always" for a tool/target will be re-prompted next time.

**Suggested change:** Add a `reject_always` decision constant and a deny-list cache in the permission manager keyed by (sessionID, tool, target). Auto-deny matching requests without re-prompting. Clear on session close (like the allow cache).

**Affected files:** `internal/permissions/`, `internal/acp/transport.go` (RequestPermission).

### 2.2 Pass terminal `env` variables to subprocesses

**Rationale:** `terminal/create` accepts an `env` parameter per spec. Currently ignored — the subprocess inherits the daemon environment. Agents that set specific env vars (e.g. PATH additions, API keys) won't have them honored.

**Suggested change:** In `CreateTerminal` (`internal/acp/terminal.go:112`), build an `exec.Cmd` environment from the daemon's env overlaid with the agent-supplied variables. Pass to `shell.Executor`.

**Affected files:** `internal/acp/terminal.go`, `internal/shell/` (executor env support).

### 2.3 Capture terminal `signal` on exit

**Rationale:** `terminal/wait_for_exit` returns both exitCode and signal. Currently only exitCode is set; signal is always nil. Processes killed by signal report incomplete info.

**Suggested change:** In the terminal goroutine (`terminal.go:159-189`), detect signal termination (e.g. via `exec.ExitError`'s `ProcessState.Sys()` on Unix) and populate `TerminalExitStatus.Signal`.

**Affected files:** `internal/acp/terminal.go`.

### 2.4 Pin the protocol version

**Rationale:** `Initialize()` doesn't set `protocolVersion` explicitly, relying on the SDK default. Pinning to a known version prevents silent behavior changes when the SDK upgrades.

**Suggested change:** Set `ProtocolVersion` explicitly in the `InitializeRequest` in `transport.go:458`. Track which spec version (v1 vs v2 draft) the SDK targets and document it.

**Affected files:** `internal/acp/transport.go`.

## Priority 3: Low Impact / Future

### 3.1 Provision MCP servers to the agent

**Rationale:** `session/new` and `session/load` pass an empty McpServers list. The spec allows the client to provision MCP servers. This would let the app expose additional tools (e.g. Context7, GitHub) to agents.

**Suggested change:** Add an MCP server config to workspace/session settings. Build the `[]acp.McpServer` from config and pass it in `NewSession`/`LoadSession`.

**Affected files:** `internal/acp/transport.go`, `internal/config/`, `internal/server/` (config API).

### 3.2 Advertise `elicitation` capability (v2 draft)

**Rationale:** v2 draft supports `elicitation: {form:{}, url:{}}` for structured user input. Not currently advertised.

**Suggested change:** When targeting v2, add elicitation to `ClientCapabilities` and implement the elicitation handler methods. Low priority — adopt when the v2 spec stabilizes and agents support it.

**Affected files:** `internal/acp/transport.go`, new elicitation handler.

### 3.3 Reconcile sessions via `session/list`

**Rationale:** Sessions are tracked client-side only. If the agent process restarts independently, the client's session map may drift from the agent's actual sessions.

**Suggested change:** On transport restart, call `session/list` (if the agent supports it) and reconcile. Low priority given the current architecture kills and restarts the whole process.

## Implementation Notes

- Recommendations 1.1 and 1.3 are interrelated — both touch the Prompt content-block construction. Implement 1.1 first (the structured-blocks infrastructure), then 1.3 builds on it.
- 1.2 is independent and low-risk — can be done in isolation.
- Priority 2 items are each small, isolated changes suitable for individual PRs.
- Run `go test ./internal/acp/...` and `go vet ./...` after each change. Update `docs/STATUS.md` and `docs/reference/acp/responsibilities.md` to reflect implemented items.

---

See `docs/reference/acp/spec.md` for the authoritative spec and `docs/reviews/2026-07-06/acp-audit.md` for the current state analysis.
