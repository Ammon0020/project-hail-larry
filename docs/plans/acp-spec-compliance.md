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
- **MCP health UX (2026-07-12)** — Backend `GET /api/mcp/status` already existed; frontend now fetches health on mount / popout open / after toggle. `McpPopout` status dots: green healthy, red unhealthy (with error tooltip), gray disabled. Loading spinner while status is in flight.
- **P4.5 AdditionalDirectories (2026-07-12)** — Other registered workspaces are passed as `additionalDirectories` on `session/new` and `session/load` when the agent advertises the capability. Client-side multi-root: `ReadTextFile`/`WriteTextFile` resolve absolute paths under any registered workspace; terminal cwd accepts any registered workspace root. Paths outside all registered workspaces still rejected.

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

**Status:** ✅ Done (promoted to P4.4, implemented there).

**Rationale:** Sessions are tracked client-side only. If the agent process restarts independently, the client's session map may drift from the agent's actual sessions.

**Suggested change:** On transport restart, call `session/list` and reconcile. `Agent.ListSessions()` has been stable in the SDK since v0.11.7 and the method is available on our `t.conn` — it just isn't wired into the restart path. (Promoted to P4.4 as near-term; see below.)

## Priority 4: SDK Feature Adoption

These are features available in `coder/acp-go-sdk v0.13.5` (or the TypeScript SDK) that we are not yet using. Split into near-term (low risk, clear value) and future (unstable, larger surface, or no current use case).

### Near-term

#### 4.1 Adopt `@agentclientprotocol/sdk` for frontend types

**Status:** ✅ Done (scoped).

**Rationale:** The frontend (`web/src/types/index.ts`) hand-rolls TypeScript types mirroring the ACP structs. The official `@agentclientprotocol/sdk` package ships the canonical types, method constants, type guards, and `PROTOCOL_VERSION`.

**What was done:** Investigation revealed `web/src/types/index.ts` contains **app-specific view-models**, not raw ACP wire types. The architecture rule "UI never communicates directly with agent implementations" means the frontend consumes backend-projected events, not ACP `SessionUpdate`/`SessionNotification` payloads. There are no pure ACP types in the file to replace — `AppEvent`, `Session`, `Agent`, `Attachment`, etc. are all app-specific projections with fields the SDK types don't have (`workspaceId`, `attachments`, `toolKind`, `agentId`, `modelId`, etc.).

The one practical adoption: typed `AppEvent.stopReason` as a local `StopReason` union matching the 5 ACP spec values (`end_turn`, `max_tokens`, `max_turn_requests`, `refusal`, `cancelled`). Removed vestigial `tool_use` handling (not in the ACP spec) and added `max_turn_requests` label in `ChatMessageItem.tsx`. Kept the type local rather than importing from the SDK to avoid adding the `@agentclientprotocol/sdk` + `zod` peer dependency for a single union type, consistent with the architecture rule.

**Affected files:** `web/src/types/index.ts` (added `StopReason` type, retyped `AppEvent.stopReason`), `web/src/components/ChatMessageItem.tsx` (updated `stopReasonLabel` signature and switch cases).

#### 4.2 Add explicit `Validate()` calls on constructed requests

**Status:** ✅ Done.

**Rationale:** Every SDK request type (`PromptRequest`, `InitializeRequest`, `NewSessionRequest`, etc.) exposes a `Validate()` method that catches malformed requests before they hit the wire. We currently construct requests and send them without validating.

**What was done:** Added `Validate()` calls on `InitializeRequest`, `NewSessionRequest`, `LoadSessionRequest`, `DeleteSessionRequest`, `PromptRequest`, and `ListSessionsRequest` in `transport.go`, returning any validation error before dispatching.

**Affected files:** `internal/acp/transport.go`.

#### 4.3 Use SDK tool content helpers

**Status:** ✅ No-op (not applicable).

**Rationale:** The SDK provides `acp.ToolContent()`, `acp.ToolDiffContent()`, and `acp.ToolTerminalRef()` to simplify tool-call **response** construction. These are sender-side helpers for agents constructing tool-call content to send to clients.

**Why no-op:** We are the ACP **client**, not the agent. `transport.go` only **consumes** tool-call updates from the agent (via `SessionUpdate` callbacks); it never constructs tool-call content. The helpers have no applicable call site.

#### 4.4 Reconcile sessions via `session/list` (promoted from P3.3)

**Status:** ✅ Done.

**Rationale:** `Agent.ListSessions()` is stable as of SDK v0.11.7 and the method is available on our transport's `t.conn`. On transport restart we previously tried `LoadSession` with the persisted ID and fell back to `NewSession` on failure. Calling `session/list` first (when the agent supports it) lets us skip the doomed `LoadSession` RPC when the session is known to be gone.

**What was done:**
- Added `Transport.ListSessions(ctx)` — calls `t.conn.ListSessions` filtered by cwd, returns `[]acp.SessionInfo`.
- Added `ListSessions` to the `transportLike` interface and mock transport.
- Modified `resolveACPSession` to reconcile: when the agent supports both `loadSession` and `sessionCapabilities.list`, it calls `ListSessions` first. If the persisted ID is not in the list, it skips `LoadSession` and goes straight to `NewSession`. If `ListSessions` fails, it falls back to the legacy try-load-then-new flow.
- Added 5 new test cases covering: list confirms session exists, list says session gone, list returns empty, list fails (fallback), list capability without load capability.

**Affected files:** `internal/acp/transport.go`, `internal/acp/acp.go`, `internal/acp/lifecycle_test.go`.

**Affected files:** `internal/acp/transport.go`, `internal/acp/acp.go` (restart path).

### Future

#### 4.5 AdditionalDirectories support

**Status:** ✅ Done (2026-07-12).

**Rationale:** Added in SDK v0.13.5. Lets agents access files outside the primary workspace root (multi-root workspaces, monorepo subprojects).

**What was done:**
- `collectAdditionalDirsLocked` gathers absolute paths of all registered workspaces except the session primary; capability-gated on `SessionCapabilities.AdditionalDirectories`.
- `NewSession`/`LoadSession` accept and send `AdditionalDirectories`.
- Client multi-root: `resolveWorkspaceFile` for read/write, `resolveCwdMulti` for terminals. Paths outside every registered workspace still rejected by `safeJoin`.
- 28 new tests in `additional_dirs_test.go`. No frontend "extra folders" UI — dirs derived from registered workspaces.

**Affected files:** `internal/acp/transport.go`, `internal/acp/acp.go`, `internal/acp/terminal.go`, `internal/acp/additional_dirs_test.go`, `lifecycle_test.go`.

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

**Status:** ✅ Done (2026-07-12).

**Rationale:** `unstable_listProviders`, `unstable_setProvider`, `unstable_disableProvider` enable dynamic provider config (e.g. switching API keys/models at runtime).

**What was done:** Transport gains `SupportsProviders`/`ListProviders`/`SetProvider`/`DisableProvider` (capability captured from `AgentCapabilities.Providers` at Initialize). Client methods are session-scoped, mirroring `SwitchModel` (lazy transport start, gate on `ErrProvidersUnsupported`). REST `GET/PUT/DELETE /api/sessions/{id}/providers[/{providerId}]` — Required-provider guard + apiType validation (anthropic/openai/azure/vertex/bedrock) in the handler; unsupported → 501, session-not-found → 404. Frontend: capability-gated "Providers (advanced)" subsection in Settings → General.

**Affected files:** `internal/acp/transport.go`, `internal/acp/acp.go`, `internal/interfaces/interfaces.go`, `internal/server/providers.go`, `internal/server/server.go`, `web/src/lib/api.ts`, `web/src/components/SettingsPanel.tsx`, `web/src/components/EditorPane.tsx`, `web/src/App.tsx`.

#### 4.12 ACP-inspector integration

**Rationale:** `venikman/ACP-inspector` is a community protocol-validation tool. Useful for testing our client against the spec.

**Suggested change:** Add an integration test or dev script that runs the daemon under ACP-inspector and asserts no protocol violations.

**Affected files:** `docs/`, test harness (new).

## Implementation Notes

- Priority 1, Priority 2, P4 near-term (4.1–4.5), and provider management (4.11) are complete. Remaining Priority 4 future items: session fork/resume/close, elicitation, NES, MCP-over-ACP, audio, ACP-inspector.
- 1.1 and 1.3 were interrelated and implemented together (structured-blocks infrastructure first, then open-files middleware on top).
- Priority 2 items were each small, isolated changes.
- Run `go test ./internal/acp/...` and `go vet ./...` after each change. Update `docs/STATUS.md` and `docs/reference/acp/responsibilities.md` to reflect implemented items.

---

See `docs/reference/acp/spec.md` for the authoritative spec, `docs/reviews/2026-07-06/acp-audit.md` for the original audit, and `docs/reference/acp/responsibilities.md` (now includes an "SDK Features Not Yet Adopted" table). SDK in use: `coder/acp-go-sdk v0.13.5` (Go) and `@agentclientprotocol/sdk` (TypeScript, candidate for P4.1).
