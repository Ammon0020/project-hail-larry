# ACP Spec Compliance Plan

This plan addresses the deviations and gaps identified in the ACP audit (`docs/reviews/2026-07-06/acp-audit.md`), ordered by impact. Each item includes the rationale, the suggested change, and the affected files.

## Completed

Priority 1 + 2 (all 7 items) are implemented (2026-07-08), along with several follow-up fixes:

- **1.1 Structured resource ContentBlocks** — Context sent as `resource` blocks (uri, mimeType, text) instead of flattened markdown; `ResourceLinkBlock + TextBlock` fallback for agents without `embeddedContext`.
- **1.2 Stop reason surfaced** — `stopReason` from `session/prompt` is captured and forwarded in the final `StreamUpdate` event; UI renders non-normal reasons (max_tokens → "hit token limit", refusal → "refused").
- **1.3 Open files / editor selection as resource blocks** — `OpenFilesResourceMiddleware` sends open-file contents and selection with every prompt; per-file (32 KiB) and aggregate (128 KiB) caps.
- **2.1 `reject_always` auto-deny** — Deny cache keyed by `(sessionID, tool, target)` mirrors the allow cache; auto-denies without re-prompting.
- **2.2 Terminal `env` variables** — Agent-supplied env overlaid on the daemon environment via `shell.Executor.WithEnv`.
- **2.3 Terminal `signal` on exit** — Signal termination detected via `syscall.WaitStatus`; signal name populated in `TerminalExitStatus.Signal`.
- **2.4 Protocol version pinned** — `ProtocolVersion` set explicitly to `acp.ProtocolVersionNumber` (v1) in `InitializeRequest`.
- **ACPClient interface completed** — Filled in the 5 missing methods on `interfaces.ACPClient` so the contract matches the implementation.
- **Shell command fallback** — Agents that send an unparsed shell string as `params.Command` with empty `params.Args` (e.g. devstral-small) are routed through `sh -c` / `cmd /c` in `CreateTerminal`.
- **Markdown export endpoint** — `GET /api/sessions/{id}/export` returns a markdown transcript instead of raw JSON.
- **Null-byte sanitization for resource blocks** — Resource text is stripped of null bytes before sending to prevent "embedded null byte" JSON-RPC errors on binary files.

## Priority 1: High Impact

### 1.1 Send context as structured resource ContentBlocks instead of flattened text ✅

**Rationale:** The spec sends file context as `resource` ContentBlocks (uri, mimeType, text) in the `session/prompt` array. The current implementation flattens workspace context into a markdown text string prepended to the user prompt. This loses structure — the agent can't distinguish context from user input or reference files by URI.

**Suggested change:** Modify `Transport.Prompt()` (`internal/acp/transport.go:528-561`) to accept structured context items and build `acp.ContentBlock` resource blocks alongside the text block. Update the pipeline (`internal/acp/context.go`) to return a list of resource blocks (file path, mimeType, content) rather than a single markdown string. Keep the text-block fallback for agents that don't advertise resource handling.

**Affected files:** `internal/acp/transport.go`, `internal/acp/context.go`, `internal/acp/acp.go` (SendPrompt pipeline integration).

### 1.2 Surface the agent's stop reason ✅

**Rationale:** The agent responds to `session/prompt` with a `stopReason` (end_turn, tool_use, max_tokens, etc.). The current code discards it (`_, err := t.conn.Prompt(...)`). The UI can't distinguish "turn ended naturally" from "hit token limit" from "stopped for tool use."

**Suggested change:** Capture the `PromptResponse` result from `t.conn.Prompt()` in `transport.go:556`. Return the stop reason to the caller. In `acp.go` SendPrompt goroutine (`acp.go:456-468`), include the stop reason in the final StreamUpdate event (add a `StopReason` field to the Event struct) so the frontend can display it.

**Affected files:** `internal/acp/transport.go` (Prompt signature + return), `internal/acp/acp.go` (SendPrompt), `internal/interfaces/` (Event struct), frontend chat components.

### 1.3 Send open files / editor selection as resource blocks with each prompt ✅

**Rationale:** The client owns editor state. The spec's intent is that relevant open files and selected code are sent as resource ContentBlocks so the agent has immediate context. Currently only a file-tree text listing is injected on the first prompt.

**Suggested change:** Add an "open files" context provider that reads the editor's open file list and current selection from the frontend (via the existing WebSocket state or a new REST endpoint). Include these as resource blocks in every prompt, not just the first. Cap by size to avoid blowing the context window.

**Affected files:** `internal/acp/context.go` (new middleware), `internal/acp/transport.go` (Prompt), frontend (send open-file state), `internal/server/` (endpoint to query open files).

## Priority 2: Medium Impact

### 2.1 Implement `reject_always` auto-deny ✅

**Rationale:** The permission policy caches `allow_always`/`allow_session` but not `reject_always`. A user who picks "reject always" for a tool/target will be re-prompted next time.

**Suggested change:** Add a `reject_always` decision constant and a deny-list cache in the permission manager keyed by (sessionID, tool, target). Auto-deny matching requests without re-prompting. Clear on session close (like the allow cache).

**Affected files:** `internal/permissions/`, `internal/acp/transport.go` (RequestPermission).

### 2.2 Pass terminal `env` variables to subprocesses ✅

**Rationale:** `terminal/create` accepts an `env` parameter per spec. Currently ignored — the subprocess inherits the daemon environment. Agents that set specific env vars (e.g. PATH additions, API keys) won't have them honored.

**Suggested change:** In `CreateTerminal` (`internal/acp/terminal.go:112`), build an `exec.Cmd` environment from the daemon's env overlaid with the agent-supplied variables. Pass to `shell.Executor`.

**Affected files:** `internal/acp/terminal.go`, `internal/shell/` (executor env support).

### 2.3 Capture terminal `signal` on exit ✅

**Rationale:** `terminal/wait_for_exit` returns both exitCode and signal. Currently only exitCode is set; signal is always nil. Processes killed by signal report incomplete info.

**Suggested change:** In the terminal goroutine (`terminal.go:159-189`), detect signal termination (e.g. via `exec.ExitError`'s `ProcessState.Sys()` on Unix) and populate `TerminalExitStatus.Signal`.

**Affected files:** `internal/acp/terminal.go`.

### 2.4 Pin the protocol version ✅

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

**Suggested change:** On transport restart, call `session/list` and reconcile. `Agent.ListSessions()` has been stable in the SDK since v0.11.7 and the method is available on our `t.conn` — it just isn't wired into the restart path. (Promoted to P4.4 as near-term; see below.)

## Priority 4: SDK Feature Adoption

These are features available in `coder/acp-go-sdk v0.13.5` (or the TypeScript SDK) that we are not yet using. Split into near-term (low risk, clear value) and future (unstable, larger surface, or no current use case).

### Near-term

#### 4.1 Adopt `@agentclientprotocol/sdk` for frontend types

**Rationale:** The frontend (`web/src/types/index.ts`) hand-rolls TypeScript types mirroring the ACP structs. The official `@agentclientprotocol/sdk` package (4.6M weekly downloads) ships the canonical types, method constants, type guards, and `PROTOCOL_VERSION`. Using it eliminates type drift.

**Suggested change:** Add `@agentclientprotocol/sdk` as a dependency; replace the hand-rolled ACP types in `web/src/types/index.ts` with imports from the SDK. Keep app-specific types (Session, Event, etc.) as-is.

**Affected files:** `web/package.json`, `web/src/types/index.ts`, any components importing the replaced types.

#### 4.2 Add explicit `Validate()` calls on constructed requests

**Rationale:** Every SDK request type (`PromptRequest`, `InitializeRequest`, `NewSessionRequest`, etc.) exposes a `Validate()` method that catches malformed requests before they hit the wire. We currently construct requests and send them without validating.

**Suggested change:** Call `Validate()` on each constructed request in `transport.go` before sending; return any validation error instead of dispatching.

**Affected files:** `internal/acp/transport.go`.

#### 4.3 Use SDK tool content helpers

**Rationale:** The SDK provides `acp.ToolContent()`, `acp.ToolDiffContent()`, and `acp.ToolTerminalRef()` to simplify tool-call response construction. The transport currently builds these structs by hand.

**Suggested change:** Replace hand-rolled tool-content struct construction in `transport.go`'s tool-call update handling with the SDK helpers.

**Affected files:** `internal/acp/transport.go`.

#### 4.4 Reconcile sessions via `session/list` (promoted from P3.3)

**Rationale:** `Agent.ListSessions()` is stable as of SDK v0.11.7 and the method is available on our transport's `t.conn`. On transport restart we currently assume the client-side session map is authoritative; calling `session/list` would let us reconcile (drop stale IDs, surface orphaned sessions).

**Suggested change:** After `Initialize` on a restart path, call `t.conn.ListSessions(ctx)` and reconcile the in-memory session map. Tolerate agents that don't support it (treat as no-op).

**Affected files:** `internal/acp/transport.go`, `internal/acp/acp.go` (restart path).

### Future

#### 4.5 AdditionalDirectories support

**Rationale:** Added in SDK v0.13.5. Lets agents access files outside the primary workspace root (multi-root workspaces, monorepo subprojects).

**Suggested change:** Expose an "additional directories" config per workspace/session; populate `AdditionalDirectories` on `NewSession`/`LoadSession`.

**Affected files:** `internal/acp/transport.go`, `internal/config/`, `internal/server/` (config API).

#### 4.6 Session fork / resume / close

**Rationale:** Agents advertise `SessionCapabilities` (fork, resume, close) but we only handle `loadSession` and `delete`.

**Suggested change:** Read `SessionCapabilities` from the initialize response and wire fork/resume/close handlers when advertised.

**Affected files:** `internal/acp/transport.go`, `internal/acp/acp.go`.

#### 4.7 Audio content blocks

**Rationale:** `acp.AudioBlock()` helper is available. No current use case — voice input/output is out of scope.

**Affected files:** `internal/acp/transport.go` (when adopted).

#### 4.8 Elicitation (v2 draft)

**Rationale:** `unstable_createElicitation` / `unstable_completeElicitation` enable structured user input (forms, URL prompts). Currently v2 draft — adopt when the spec stabilizes and agents support it.

**Affected files:** `internal/acp/transport.go`, new elicitation handler, frontend prompt UI.

#### 4.9 Next Edit Suggestions (NES)

**Rationale:** Inline code completion via unstable SDK methods. Complex feature; defer until stable.

**Affected files:** `internal/acp/transport.go`, frontend editor integration.

#### 4.10 MCP-over-ACP

**Rationale:** `mcp/connect`, `mcp/disconnect`, `mcp/message` would let the agent talk to MCP servers brokered by the client. Needs an MCP server config UI first.

**Affected files:** `internal/acp/transport.go`, `internal/config/`, `internal/server/`, frontend settings.

#### 4.11 Provider management

**Rationale:** `unstable_listProviders`, `unstable_setProvider`, `unstable_disableProvider` enable dynamic provider config (e.g. switching API keys/models at runtime).

**Affected files:** `internal/acp/transport.go`, frontend settings.

#### 4.12 ACP-inspector integration

**Rationale:** `venikman/ACP-inspector` is a community protocol-validation tool. Useful for testing our client against the spec.

**Suggested change:** Add an integration test or dev script that runs the daemon under ACP-inspector and asserts no protocol violations.

**Affected files:** `docs/`, test harness (new).

## Implementation Notes

- Priority 1 and Priority 2 are complete (see "Completed" above). The current focus is Priority 4 (SDK feature adoption) — start with the near-term items (4.1–4.4), which are low-risk and isolated.
- 1.1 and 1.3 were interrelated and implemented together (structured-blocks infrastructure first, then open-files middleware on top).
- Priority 2 items were each small, isolated changes.
- Run `go test ./internal/acp/...` and `go vet ./...` after each change. Update `docs/STATUS.md` and `docs/reference/acp/responsibilities.md` to reflect implemented items.

---

See `docs/reference/acp/spec.md` for the authoritative spec, `docs/reviews/2026-07-06/acp-audit.md` for the original audit, and `docs/reference/acp/responsibilities.md` (now includes an "SDK Features Not Yet Adopted" table). SDK in use: `coder/acp-go-sdk v0.13.5` (Go) and `@agentclientprotocol/sdk` (TypeScript, candidate for P4.1).
