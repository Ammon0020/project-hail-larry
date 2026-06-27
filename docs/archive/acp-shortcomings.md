# ACP Implementation Shortcomings

**Date:** 2026-06-23
**Purpose:** Handoff document for another developer to address gaps in our ACP client implementation.

## Resolution Status (2026-06-23)

Addressed in this pass (see `docs/specs/backend-spec.md` + `docs/plans/acp-stability.md`):

- **#1 Client capabilities** — FIXED. `transport.Initialize` now advertises `Fs.{ReadTextFile,WriteTextFile}` and `Terminal: true`.
- **#2 Terminal methods** — FIXED. Implemented in `internal/acp/terminal.go`, backed by `internal/shell`, streaming shell events.
- **#3 SessionUpdate types** — FIXED. Thoughts (`Thought` flag), plans (`PlanUpdated`), and richer tool calls now emitted.
- **#4 Tool call detail** — FIXED. Events carry `ToolKind`, `ToolCallID`, `Target`, `Command`, and content/diff summary.
- **#5 Model ID to session** — N/A in SDK v0.13.5 (`NewSessionRequest` has no `ModelId`). Replaced by client-side model/agent **rebind** (`RebindSession`, `PATCH /api/sessions/{id}`) that preserves conversation history.
- **#6 Close/load/resume** — PARTIAL. `CancelSession` now sends ACP cancel (keeps process); graceful `session/close` on shutdown still deferred (kill is reliable). Load/resume deferred.
- **#7 Agent stderr** — FIXED. Captured into a capped `ringBuffer`; tail surfaced in `AgentExited`.
- **#8 Frontend shell events** — FIXED. `ChatMessageItem` renders shell start/output/complete cards.
- **#9 Stale model lists** — unchanged (low priority).

**Also fixed (not in original list):** the broken permission path — `PermissionManager` now emits `PermissionRequested` via a callback; the UI renders dynamic option buttons and responds with the correct `requestId`. Conversations now persist across restarts with rename/delete.

---


## Background

Our app uses the [Agent Client Protocol (ACP)](https://agentclientprotocol.com/) implemented via `coder/acp-go-sdk` for communication between editors (clients) and AI coding agents. The architecture is **client-server**: the agent process plans and proposes actions; the client (our Go daemon) owns the filesystem, shell execution, and permissions, and executes approved actions on the agent's behalf.

Per the spec ([overview](https://agentclientprotocol.com/protocol/overview)):
> "Clients manage the environment, handle user interactions, and control access to resources."

Per `AGENTS.md`:
> "Agents plan and propose; the client executes approved actions."

---

## Issue 1: Client capabilities not advertised (Critical)

**File:** `internal/acp/transport.go:198-206`

During the ACP `initialize` handshake, we send an empty `ClientCapabilities{}`:

```go
func (t *Transport) Initialize(ctx context.Context) (acp.InitializeResponse, error) {
    return t.conn.Initialize(ctx, acp.InitializeRequest{
        ClientInfo: &acp.Implementation{
            Name:    "LocalAgentInterface",
            Version: "1.0",
        },
        ClientCapabilities: acp.ClientCapabilities{}, // <-- empty!
    })
}
```

Per the [initialization spec](https://agentclientprotocol.com/protocol/initialization), capabilities default to `false` when omitted:
> "Clients and Agents MUST treat all capabilities omitted in the `initialize` request as UNSUPPORTED."

This means we're telling agents:
- `fs.readTextFile: false` — agents won't try to read files via our client
- `fs.writeTextFile: false` — agents won't try to write files via our client
- `terminal: false` — agents won't try to run shell commands via our client

**Impact:** Agents that check capabilities (which the spec says they MUST) will skip all filesystem and terminal operations. This is likely why real agents like Codex produce no output — they see no capabilities and can't do anything useful.

**Fix:** Advertise the capabilities we support:

```go
ClientCapabilities: acp.ClientCapabilities{
    Fs: acp.FileSystemCapabilities{
        ReadTextFile: true,
        WriteTextFile: true,
    },
    Terminal: true, // once terminal methods are implemented (Issue 2)
},
```

Only set `Terminal: true` after the terminal methods are actually implemented (Issue 2). Set `Fs` capabilities now since `ReadTextFile` and `WriteTextFile` are already implemented.

The same issue exists in `autodetect.go:160-163` where we also send empty `ClientCapabilities{}` during the probe handshake. This is less critical for autodetect (we only need `Initialize` + `UnstableListProviders`), but should be fixed for consistency.

---

## Issue 2: Terminal methods not implemented (Critical)

**File:** `internal/acp/transport.go:144-162`

All five terminal methods return errors:

```go
func (c *acpClientImpl) CreateTerminal(...) (acp.CreateTerminalResponse, error) {
    return acp.CreateTerminalResponse{}, fmt.Errorf("terminals not supported yet")
}
// KillTerminal, TerminalOutput, ReleaseTerminal, WaitForTerminalExit — same
```

Per the [terminal spec](https://agentclientprotocol.com/protocol/terminals):
> "The terminal methods allow Agents to execute shell commands within the Client's environment."

The ACP terminal flow is:
1. Agent calls `terminal/create` with a command → client starts the process, returns a `TerminalId` immediately
2. Agent calls `terminal/output` to get current stdout/stderr (non-blocking)
3. Agent calls `terminal/wait_for_exit` to block until the command finishes
4. Agent calls `terminal/kill` to terminate without releasing
5. Agent calls `terminal/release` to kill + release resources

**Impact:** Agents cannot run shell commands (build, test, lint, etc.). This severely limits what agents can do — most coding agents need to run commands to inspect the project, execute tests, etc.

**Fix:** We already have `internal/shell/shell.go` with an `Executor` that runs commands in a workspace directory, including a `RunAsync` method that streams output via callbacks. Wire the terminal methods to this executor:

1. Maintain a `map[string]*terminalEntry` in `acpClientImpl` (terminal ID → process + output buffer)
2. `CreateTerminal` — use `shell.NewExecutor(workspacePath)` + `RunAsync` to start the command, store the process, return a generated terminal ID
3. `TerminalOutput` — return the current buffered output and exit status (if done)
4. `WaitForTerminalExit` — block until the process exits, then return exit code + output
5. `KillTerminal` — send kill signal to the process, keep the terminal entry
6. `ReleaseTerminal` — kill if still running, delete the terminal entry

The `shell.Executor.RunAsync` already handles OS-specific shell invocation (`cmd /C` on Windows, `sh -c` on Unix) and streams stdout/stderr via callbacks.

---

## Issue 3: SessionUpdate doesn't handle all update types (Medium)

**File:** `internal/acp/transport.go:25-63`

The `SessionUpdate` handler only processes `AgentMessageChunk` and `ToolCall`/`ToolCallUpdate`. Several update types are silently dropped:

| Update Type | Current Behavior | Should Do |
|---|---|---|
| `AgentMessageChunk` | Emits `StreamUpdate` event | ✅ Correct |
| `AgentThoughtChunk` | Silently ignored | Emit as a thought/thinking event (UI can show "thinking..." indicator) |
| `ToolCall` (start) | Emits `ToolStarted` | ✅ Correct, but doesn't capture tool kind, raw input, or locations |
| `ToolCallUpdate` | Emits `ToolCompleted` | ✅ Partial — doesn't capture tool output content, diff, or terminal references |
| `Plan` | Silently ignored | Emit as a plan event (UI can show task checklist) |
| `UserMessageChunk` | Silently ignored | Usually fine (we already emitted the prompt), but could be used for echo confirmation |

**Impact:** The UI doesn't show agent thoughts, plans, or detailed tool call information (file paths, command inputs, diffs). The chat experience is degraded compared to what the agent is actually sending.

**Fix:** Extend the `SessionUpdate` switch to handle `AgentThoughtChunk` and `Plan`. Enrich `ToolStarted`/`ToolCompleted` events with tool kind, input, output, and file locations from the ACP update fields. This may require adding new fields to `interfaces.Event` (e.g., `ToolKind`, `ToolInput`, `ToolOutput`, `Locations`).

---

## Issue 4: Tool call events lack detail (Medium)

**File:** `internal/acp/transport.go:42-57`

Tool call events are minimal:

```go
case u.ToolCall != nil:
    c.callbacks.OnEvent(interfaces.Event{
        Type:      interfaces.EventToolStarted,
        SessionID: c.sessionID,
        Tool:      u.ToolCall.Title,  // only title, no kind/input/locations
    })
case u.ToolCallUpdate != nil:
    c.callbacks.OnEvent(interfaces.Event{
        Type:      interfaces.EventToolCompleted,
        SessionID: c.sessionID,
        Summary:   status,  // only status string, no output/diff
    })
```

The ACP `ToolCall` struct has rich fields we're dropping:
- `Kind` — read, edit, execute, search, etc. (determines UI icon)
- `Status` — pending, in_progress, completed, failed
- `Locations` — file paths affected
- `RawInput` — the command or edit content
- `RawOutput` — command output or edit result

The `ToolCallUpdate` struct has:
- `Content` — tool call content blocks (text, diff, terminal references)
- `RawOutput` — structured output data

**Impact:** The frontend `ChatMessageItem.tsx` can only show a generic wrench icon and the tool title. It can't show what file is being edited, what command is being run, or the output/diff.

**Fix:** Add fields to `interfaces.Event` for tool kind, input, output, and locations. Populate them from the ACP update structs. Update the frontend to render them (e.g., show file paths, command text, diffs).

---

## Issue 5: Model ID not passed to agent session (Medium)

**File:** `internal/acp/transport.go:208-217`

When creating a new ACP session, we don't pass the model ID:

```go
func (t *Transport) NewSession(ctx context.Context, cwd string) (string, error) {
    result, err := t.conn.NewSession(ctx, acp.NewSessionRequest{
        Cwd:        cwd,
        McpServers: []acp.McpServer{},
    })
    ...
}
```

The `acp.NewSessionRequest` struct has a `ModelId` field. We collect the model ID from the user (via the frontend agent/model selector) and store it in `Session.ModelID`, but never pass it to the agent. The agent has no way to know which model the user selected.

**Impact:** Agents that support multiple models (e.g., Codex with GPT-4o vs GPT-4 Turbo) will use their default model, ignoring the user's selection.

**Fix:** Thread the `modelID` through `Transport.NewSession` and set it in the `NewSessionRequest`:

```go
func (t *Transport) NewSession(ctx context.Context, cwd, modelID string) (string, error) {
    result, err := t.conn.NewSession(ctx, acp.NewSessionRequest{
        Cwd:        cwd,
        ModelId:    acp.Ptr(acp.ModelId(modelID)),
        McpServers: []acp.McpServer{},
    })
    ...
}
```

---

## Issue 6: No session close/load/resume support (Low)

**File:** `internal/acp/transport.go`

We don't call `session/close`, `session/load`, or `session/resume` on the agent. When a user closes a chat session, we kill the agent process instead of gracefully closing the ACP session.

**Impact:** Agents can't clean up resources, save session state, or support session resumption. This is fine for v1 but limits future features like session history replay.

**Fix:** Call `conn.CloseSession()` before killing the process in `Transport.Close()`. Add `LoadSession`/`ResumeSession` methods when session persistence is needed.

---

## Issue 7: Agent stderr inherited by daemon (Low)

**File:** `internal/acp/transport.go:178`

```go
t.cmd.Stderr = os.Stderr
```

Agent process stderr is piped directly to the daemon's stderr. This means any agent diagnostic output (warnings, deprecation notices, "stdin is not a terminal" errors) appears in the daemon log. This was the source of the "stdin is not a terminal" noise.

**Impact:** Daemon logs can be polluted with agent stderr output, making it harder to diagnose daemon-specific issues.

**Fix:** Consider capturing agent stderr to a per-session buffer or file, or filtering known noise patterns. The autodetect probe already discards stderr (`io.Discard`), but real sessions inherit it.

---

## Issue 8: Frontend doesn't render shell command events (Low)

**Files:** `web/src/components/ChatMessageItem.tsx`, `internal/interfaces/interfaces.go:31-33`

We define event types for shell commands (`ShellCommandStarted`, `ShellOutputStreamed`, `ShellCommandCompleted`) in the backend, but the frontend `ChatMessageItem` has no case for them — they fall through to the `default: return null`.

**Impact:** When terminal methods are implemented (Issue 2) and shell events are emitted, they won't appear in the UI.

**Fix:** Add cases in `ChatMessageItem.tsx` for shell command events — show the command, stream output, and exit code in a terminal-style card.

---

## Issue 9: No `UnstableListProviders` in real sessions (Low)

**File:** `internal/acp/acp.go:117-206`

During `CreateSession`, we spawn the agent and do `Initialize` + `NewSession`, but never call `UnstableListProviders`. We rely on autodetect (run once at startup) for model discovery. If an agent's available models change after startup (e.g., new models installed), we won't see them until daemon restart.

**Impact:** Stale model lists. Low severity since models rarely change mid-session.

**Fix:** Optionally call `UnstableListProviders` during `CreateSession` or cache with a TTL. Not urgent.

---

## Priority Summary

| Priority | Issue | Effort |
|---|---|---|
| **Critical** | #1 — Advertise client capabilities | Small (a few lines) |
| **Critical** | #2 — Implement terminal methods | Medium (wire shell.go to ACP terminal interface) |
| **Medium** | #3 — Handle all SessionUpdate types | Medium (new event fields + UI) |
| **Medium** | #4 — Enrich tool call events | Medium (event struct changes + UI) |
| **Medium** | #5 — Pass model ID to agent session | Small (thread modelID through) |
| **Low** | #6 — Session close/load/resume | Medium |
| **Low** | #7 — Agent stderr handling | Small |
| **Low** | #8 — Frontend shell event rendering | Small |
| **Low** | #9 — Stale model lists | Small |

## Recommended Order

1. Fix #1 first (advertise `fs` capabilities) — immediate improvement, agents can read/write files
2. Fix #5 (pass model ID) — small change, ensures correct model is used
3. Fix #2 (terminal methods) — biggest functional gap, enables shell command execution
4. Fix #3 + #4 together — richer event stream, better UX
5. Fix #8 — show shell output in UI once terminal methods are working
6. #6, #7, #9 — polish items

## Test Infrastructure

We have a mock ACP agent (`cmd/mockagent/main.go`) and integration tests (`internal/acp/integration_test.go`) that verify the full flow: session creation, prompt sending, streaming responses, and tool calls. These tests build the mock agent binary and run it as a real subprocess.

Real agent tests (`TestRealAgentDevstral`, `TestRealAgentCodex`) are skip-by-default — set `ACP_TEST_REAL=1` to run them against live agents.

After fixing the issues above, update the mock agent to exercise terminal methods and the new event types to ensure regression coverage.
