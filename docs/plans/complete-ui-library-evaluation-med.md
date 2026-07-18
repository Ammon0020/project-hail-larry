# Epic: UI Library Evaluation — ACP/Chat Libraries

> **Status:** Decision record. **Owner:** —. **Phase:** Unphased (research).
> Investigated 2026-07-13 to 2026-07-16. No stories yet — flesh out before working.

## Goal

Decide whether adopting any of three libraries would simplify the chat UI and
reduce maintenance. Verdict per library below. **Outcome: none adopted.** One
internal pattern refactor identified as a future candidate (no dependency).

## Context

Architecture under evaluation (Blueprint §2-3, §11):
- Go (→Rust) backend owns ACP via `coder/acp-go-sdk`, SQLite event store,
  real-filesystem writes with revision tracking + three-way merge.
- Frontend (React 19 + shadcn + Tailwind v4) renders an immutable `AppEvent`
  stream over WebSocket; multi-device sync, reconnect/resync, replay.
- ~2,600 lines of chat UI: `ChatPanel`, `ChatMessageItem`, `ConversationView`,
  `ChatComposer`, `ChatTabBar`, `chat/{ThinkingBlock,ToolExecutionBlock,
  McpPopout,WorkspaceBar}`.

Hard constraints that drive every verdict:
- **Client ownership** — host owns filesystem, shell, permissions, session
  state. Agents propose; client executes (Blueprint §2, §14).
- **Stateless UI** — server-authoritative event log; devices are thin clients
  (Blueprint §2, §11-12).
- **Multi-device** — permission prompts broadcast to all paired devices;
  first response wins (Blueprint §8).
- **No UI→agent direct communication** (Blueprint §2, AGENTS.md).

## Library 1 — Vercel AI SDK (`ai` + `@ai-sdk/react`)

**Verdict: No.** Architectural mismatch at the foundation.

Two relevant pieces, both unsuitable:

### A. ACP Community Provider (`@mcpc-tech/acp-ai-provider`)
- Community-maintained (not in `vercel/ai`), implements `LanguageModelV2`.
- Spawns agent processes via `child_process` — **Node.js only**, server-side.
- Duplicates what the Go/Rust backend already does via `coder/acp-go-sdk`.
- Limitation: model selection not yet supported.
- Not a UI library. Adopting means replacing the backend's ACP layer with Node.

### B. AI SDK Harnesses (official, v7) — closest fit, still collides
- Official adapters for Claude Code, Codex, OpenCode, Pi (all ACP agents).
- Owns session lifecycle, conversation history, streaming as UI message parts
  (`text`, `reasoning`, `tool-bash`, `tool-read`, `dynamic-tool` for
  `fileChange`/`compaction`).
- Tool approval flow: `tool-approval-request` / `tool-approval-response` parts
  with `addToolApprovalResponse()` on the client.
- `useChat()` consumes the harness stream via `toUIMessageStream()`.

**Why it doesn't fit — the sandbox:**
- Harness model is **sandbox-centric**. Adapter table:

  | Adapter      | Runtime location |
  |--------------|------------------|
  | Claude Code  | Sandbox bridge   |
  | Codex        | Sandbox bridge   |
  | OpenCode     | Sandbox bridge   |
  | Pi           | Host process     |

- "Bridge-backed harnesses require using real network sandbox like
  `@ai-sdk/sandbox-vercel`." Only Pi can use `@ai-sdk/sandbox-just-bash`
  (virtual FS, copy-on-write, writes vanish on stop).
- Our model is the opposite: agent proposes → host daemon writes the **real
  filesystem** with revision tracking, three-way merge, `FileRevisionUpdated`
  broadcast to all devices (Blueprint §2, §14).

**Three adoption paths, all net-negative:**
1. **Replace backend with Node HarnessAgent** — abandon Rust port, lose
   real-FS/revision/merge/multi-device infra. Product pivot, not UI simplification.
2. **Custom harness adapter + local sandbox over real FS** — rebuild the ACP
   layer in Node against a v1.0, fast-moving harness contract. Adds a Node
   process + translation layer. Complexity up.
3. **`useChat()` frontend only, project AppEvents → UIMessage stream** — keep
   backend, add a server-side AppEvent→UIMessage projector. Modest win on
   send/receive lifecycle (~100-150 lines), but:
   - Rendering still custom: `ChatMessageItem`, `ToolExecutionBlock`,
     `ThinkingBlock`, turn-folding all stay. IDE events
     (`FileRevisionUpdated`, `FileChangedOnDisk`, `ModelChanged`,
     `ConnectionRestarted`, `SessionResumed`) aren't message parts — second
     rendering system needed.
   - `useChat()` is one thread; our multi-tab `ChatTabBar`/`useChatTabs` stays.
   - `tool-approval-request` is single-stream request/response; our
     broadcast-first-response-wins model (Blueprint §8) needs a custom transport.
   - Two state models to reconcile: event log (sync) vs `useChat` message list.

## Library 2 — Assistant UI (`@assistant-ui/react`)

**Verdict: Best fit, but real migration cost. Spike-worthy if pursued.**

- React chat components on shadcn (matches our stack exactly).
- `ExternalStoreRuntime` wraps **your** state — you own messages, adapter
  converts `AppEvent[]` → `ThreadMessageLike[]`. Capability-based: provide
  `onCancel` → cancel button appears.
- The **OpenCode runtime** (`@assistant-ui/react-opencode`) is almost our
  architecture: external coding-agent server, SSE event stream → thread, tool
  permissions, interactive questions. Our app is "the OpenCode runtime, but for
  ACP agents."
- Pre-built primitives: Thread, Composer, MessageList with autoscroll,
  virtualization, attachments, input history, slash commands, mentions,
  branching, edit/regenerate.

**What it replaces:** `ConversationView` + `ChatMessageItem` + `ChatComposer`
inner rendering (~860 lines) → adapter + `Thread`/`Composer` + custom part
renderers.

**What stays custom (lives above the runtime):**
- `ChatPanel` orchestrator, `ChatTabBar`/`useChatTabs` multi-session tabs,
  `WorkspaceBar`, session rebind, MCP banners, profile modes — assistant-ui's
  Thread is one conversation; our tab/workspace UX is not given.
- IDE-event rendering for `FileRevisionUpdated`, `ModelChanged`,
  `ConnectionRestarted`, etc. — rendered outside the Thread.

**Costs / friction:**
- `AppEvent` → `ThreadMessageLike` conversion is non-trivial: one agent turn =
  many events (StreamUpdate chunks, ToolStarted/Completed, ShellOutputStreamed,
  PermissionRequested) folded into one assistant message with ordered parts.
  `useExternalMessageConverter` + `joinStrategy` help; rich part mapping is custom.
- **Permissions are the sharpest edge.** `onAddToolResult` is for client-side
  tool-result handoff; our permissions are server-side agent approvals
  (approve/deny a shell command). Render as custom interactive parts — doable,
  not stock.
- Branching/edit/regenerate partly no-ops: ACP agents don't "regenerate a turn"
  like an LLM API; editing means re-prompting. Some adapter semantics won't map.
- Version churn: v0.x, `unstable_` APIs, fast major cadence (v4→v7 legacy
  adapters already exist).

**Spike go/no-go criterion:** does the permission-prompt-as-interactive-part
and turn-folding conversion feel clean, or like fighting the runtime? If it
fights, the IDE-event semantics are too custom — keep the hand-rolled renderer.

**Timing:** land after the Rust port's server story stabilizes so the adapter
is written once against a stable event surface.

## Library 3 — ACP TypeScript SDK (client) (`@agentclientprotocol/sdk`)

**Verdict: No (for the UI).** Backend library; the Go/Rust backend already
does this job.

- Official TS implementation of the ACP **client** role: JSON-RPC client over
  a duplex stream (stdio), `requestPermission`/`sessionUpdate` handlers,
  `connectWith(stream, ...)`.
- That role is exactly what `internal/acp/` does via `coder/acp-go-sdk`.
- Browser can't spawn processes or do stdio — cannot run in the UI.
- Adopting in the frontend violates Blueprint §2 ("UI never talks to agents
  directly") and `types/index.ts` (frontend consumes backend-projected
  `AppEvent`s, not raw ACP wire types).
- Only relevant if porting backend Go→Node (we're porting Go→Rust). Even then
  it's a backend choice, not a UI simplification.

## Actionable Finding — Permission-as-Message-Part Pattern (no dependency)

The one valuable pattern from the AI SDK investigation, adoptable without any
library: model permission resolution as **message-part state** rather than a
side-channel Map.

**Current:** `PendingPermission[]` + `permissionResolution: Map<requestId,
'granted'|'denied'>` + `onPermissionResponse` callback, threaded through
`ConversationView` → `ChatMessageItem` as separate props.

**Pattern (from AI SDK `tool-approval-request`/`tool-approval-response`):**
permission state lives on the message/event itself — `PermissionRequested`
event carries approval state (`pending`/`granted`/`denied`/`options`), and the
renderer reads it inline. Removes the side-channel Map and the prop threading.

**Scope of a future story:** internal refactor in `ConversationView` +
`ChatMessageItem` + `ChatPanel` state. No new dependency. Folds into the
existing `AppEvent` model — `PermissionRequested` already carries `requestId`,
`tool`, `command`, `options`. Flesh out before working.

## Status

- ⬜ Decision record (this doc) — complete.
- ⬜ (Future, if pursued) assistant-ui spike: minimal `ExternalStoreRuntime`
  adapter against `AppEvent` stream, one thread, evaluate permission/turn-folding.
- ⬜ (Future, optional) Permission-as-message-part refactor story.

## References

- Blueprint: `docs/plans/Blueprint.md` §2-3, §8, §11-14
- Current chat UI: `web/src/components/ChatPanel.tsx`, `ConversationView.tsx`,
  `ChatMessageItem.tsx`, `ChatComposer.tsx`, `chat/ToolExecutionBlock.tsx`,
  `chat/ThinkingBlock.tsx`
- ACP backend: `internal/acp/acp.go`
- AI SDK Harnesses: https://ai-sdk.dev/v7/docs/ai-sdk-harnesses/overview
- AI SDK ACP provider: https://ai-sdk.dev/v7/providers/community-providers/acp
- Assistant UI ExternalStoreRuntime: https://www.assistant-ui.com/docs/runtimes/custom/external-store.md
- Assistant UI OpenCode runtime: https://www.assistant-ui.com/docs/runtimes/opencode/overview.md
- ACP TS SDK: https://github.com/agentclientprotocol/typescript-sdk
