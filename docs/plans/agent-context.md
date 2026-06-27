# Agent Context Provider

> **Status: DRAFT — Needs team review.**
> This plan has not been approved or implemented. Open questions at the bottom must be resolved before implementation begins.

## Problem

When an agent (e.g. mistral-vibe) receives a prompt like "Summarize readme.md", it has no idea what files exist in the workspace. ACP provides no directory-listing capability — only `fs/read_text_file` and `fs/write_text_file`. The agent must fall back to terminal commands (`dir`, `ls`, `find`) to discover files, which:

1. Triggers permission prompts for every exploration command
2. Adds latency (each command round-trips through the permission flow)
3. Fills the conversation with noise (shell tool cards instead of clean responses)
4. Fails on path edge cases (the agent doesn't know the workspace root, sends absolute paths that may be outside the workspace)

This was the root cause of the "ton of back and forth" in the exported `New_chat.json` conversation.

---

## mistral-vibe Framework Analysis

Cloned to `C:\Users\adama\Documents\mistral-vibe-ref` for reference. mistral-vibe has a well-structured framework for managing prompts, context, and agent behavior. Here's what it does and what we can learn.

### 1. System Prompt Assembly (`vibe/core/system_prompt.py`)

`get_universal_system_prompt()` is the central builder. It assembles sections in order:

| Section | Source | What It Provides |
|---|---|---|
| Base prompt | `prompts/cli.md` | Agent identity, instruction hierarchy, behavior rules |
| Headless mode | `_get_headless_section()` | "No human available, don't ask questions" |
| Commit signature | `_add_commit_signature()` | Co-author format for git commits |
| Model info | `config.active_model` | "Your model name is: `devstral-2`" |
| OS info | `_get_os_system_prompt()` | Platform, shell, Windows compatibility rules |
| Tool prompts | `tool_manager.available_tools` | Each tool loads a `.md` prompt file (e.g. `read.md`, `bash.md`) |
| Skills | `skill_manager` | Available skill descriptions in XML format |
| Subagents | `agent_manager` | Delegatable subagent descriptions |
| Scratchpad | `_get_scratchpad_section()` | Temp directory path, no permission prompts |
| Project context | `ProjectContextProvider` | **Absolute path + git status** (branch, main branch, status summary, recent commits) |
| AGENTS.md | `harness_files_manager` | User-level + project-level instruction files, with priority hierarchy |

Key: mistral-vibe does **not** include a file tree. It provides git status + absolute path. The agent uses its own tools (`grep`, `read`, `bash`) for file discovery.

### 2. Middleware Pipeline (`vibe/core/middleware.py`)

A `MiddlewarePipeline` runs `before_turn()` before each agent turn. Each middleware returns one of:
- **CONTINUE** — proceed normally
- **STOP** — halt (turn limit, price limit, token limit)
- **COMPACT** — trigger context compaction
- **INJECT_MESSAGE** — inject a message into the conversation

Built-in middlewares:
- `TurnLimitMiddleware` — max turns per session
- `PriceLimitMiddleware` — max $ per session
- `TokenLimitMiddleware` — max tokens per session
- `AutoCompactMiddleware` — trigger compaction at threshold
- `ContextWarningMiddleware` — warn at % of context window
- `ReadOnlyAgentMiddleware` — inject plan/chat mode reminders

This is the framework for **injecting context at specific points** in the conversation lifecycle.

### 3. Agent Profiles (`vibe/core/agents/models.py`)

Predefined profiles that override config:

| Profile | Safety | Behavior |
|---|---|---|
| `default` | Neutral | Requires approval for tool execution |
| `plan` | Safe | Read-only, auto-approves safe read tools, writes only to plan file |
| `chat` | Safe | Read-only conversational mode, bypasses tool permissions |
| `accept-edits` | Destructive | Auto-approves file edits, still asks for shell commands |
| `auto-approve` | YOLO | Auto-approves everything |
| `explore` | Safe | Subagent for codebase exploration (grep + read only) |
| `lean` | Neutral | Specialized for Lean 4 proof assistant |

Profiles are TOML files in `~/.vibe/agents/` or `.vibe/agents/`. They override: system prompt, enabled/disabled tools, tool permissions, model, compaction settings.

### 4. Skills System (`vibe/core/skills/manager.py`)

Skills are on-demand context modules, discovered from `SKILL.md` files in:
- `~/.vibe/skills/` (user-level)
- `.vibe/skills/` (project-level)
- Built-in skills

When user types `/skillname`, the skill's markdown body is injected into the prompt. Skills are listed in the system prompt as available, but their full content is only loaded on demand.

### 5. Harness Files Manager (`vibe/core/config/harness_files/`)

Central discovery system for:
- **AGENTS.md** — walks up from cwd to trust root, collecting instruction files. Closer files override distant ones.
- **Custom prompts** — `~/.vibe/prompts/` and `.vibe/prompts/` (system + compaction prompts)
- **Custom skills** — `~/.vibe/skills/` and `.vibe/skills/`
- **Custom tools** — `~/.vibe/tools/` and `.vibe/tools/`
- **Config** — `.vibe/config.toml` (project) overrides `~/.vibe/config.toml` (user)

### 6. ACP Integration (`vibe/acp/acp_agent_loop.py`)

When running as an ACP agent, mistral-vibe:
- Creates an internal `AgentLoop` (same as CLI mode)
- The `AgentLoop` builds the system prompt via `get_universal_system_prompt()`
- ACP prompts are converted to text and fed to the internal loop
- Tool overrides in `acp/tools/builtins/` wrap core tools to use ACP client methods (e.g., `read` calls `client.read_text_file()` instead of reading disk directly)
- `_build_text_prompt()` handles `ContentBlock::Text`, `ContentBlock::Resource`, and `ContentBlock::ResourceLink` — so ACP clients CAN send context via resource blocks

### 7. Compaction (`vibe/core/compaction.py`)

When context exceeds threshold:
- Summarizes conversation history
- Preserves recent user messages verbatim (within token budget)
- Injects a compaction context message with XML tags: `<previous_user_messages>`, `<compaction_summary>`

### Can We Harness mistral-vibe Directly?

**Not directly.** mistral-vibe is a Python ACP *agent* (server side). We're a Go ACP *client*. They're on opposite sides of the protocol. But we can adopt its patterns.

### What We Can Adopt

| mistral-vibe Pattern | Our Equivalent | Feasibility |
|---|---|---|
| System prompt assembly | Prepend context to first prompt | ✅ Easy — we can't set system prompt, but we can wrap the user's first message |
| Middleware pipeline | Pre-prompt hooks in `SendPrompt()` | ✅ Easy — interface with `beforePrompt()` returning inject/stop/continue |
| Agent profiles | Session modes (plan, chat, auto-approve) | ✅ Medium — we already have ACP session modes; we can add our own |
| Skills (SKILL.md) | Workspace-level context files | ✅ Medium — discover `.agent/skills/` in workspace, inject on demand |
| AGENTS.md | Already have this (our `AGENTS.md`) | ✅ Already done — we can inject it into the first prompt |
| Harness files manager | Config + prompt discovery | ✅ Medium — discover prompt templates from `~/.local-agent/prompts/` |
| Compaction | Not our job — agent handles this | ❌ Agent-side concern |
| Tool prompt files | Not applicable — agent owns its tools | ❌ Agent-side concern |

### Proposed Architecture: Context Injection Framework

Instead of a single `ContextProvider`, adopt mistral-vibe's layered approach:

```
internal/context/
  provider.go          — ContextProvider interface
  pipeline.go          — PromptPipeline (runs before each prompt)
  middleware.go        — Middleware interface (beforePrompt → inject/stop/continue)
  project_context.go   — Builds workspace context (file tree, git status, OS info)
  agents_md.go         — Discovers and loads AGENTS.md files
  skills.go            — Discovers SKILL.md files from workspace
  templates/           — Markdown prompt templates with $variable interpolation
    project_context.md
    agents_doc.md
```

**Pipeline flow:**
1. User sends prompt
2. `PromptPipeline.runBeforePrompt()` calls each middleware
3. Middlewares can: inject context (first prompt only), inject AGENTS.md, inject skill content, warn about context limits
4. If any middleware returns STOP, prompt is rejected
5. Injected context is prepended to user's prompt
6. `transport.Prompt()` sends the combined content to the agent

**Middleware interface:**
```go
type MiddlewareAction int
const (
    ActionContinue MiddlewareAction = iota
    ActionStop
    ActionInjectMessage
)

type MiddlewareResult struct {
    Action  MiddlewareAction
    Message string  // injected content (if ActionInjectMessage)
    Reason  string  // stop reason (if ActionStop)
}

type Middleware interface {
    BeforePrompt(ctx context.Context, session *SessionContext) MiddlewareResult
    Reset()
}
```

**Built-in middlewares:**
- `FirstPromptContextMiddleware` — injects workspace context (file tree, git, OS) on first prompt only
- `AgentsMdMiddleware` — injects AGENTS.md content on first prompt
- `SkillMiddleware` — injects skill content when user types `/skillname`

---

## Current Architecture

```
User types message in UI
  → POST /api/sessions/:id/prompt
  → daemon.ACPClient.SendPrompt(ctx, sessionID, content)
  → acp.Client.SendPrompt()
    → session.transport.Prompt(ctx, acpSessionID, content)
      → conn.Prompt(PromptRequest{ Prompt: [TextBlock(content)] })
```

The `content` string passes through verbatim — no context is added. The agent receives only the user's raw text.

### What We Already Have Available

- `workspace.Manager.FileTree()` — builds a recursive `[]FileNode` tree (name, type, path, children)
- `workspace.Manager` — knows the workspace path
- We could get git status via `git` commands (branch, status, recent commits)
- The `acpClientImpl` has `workspacePath` and `workspaceID`

### Where Context Injection Would Happen

In `acp.Client.SendPrompt()` at `@/internal/acp/acp.go:240`, before calling `session.transport.Prompt()`. We'd wrap the user's content with a context preamble.

---

## Implementation Plan

### Phase 1: Context Injection Framework

Create `internal/context/` package adopting mistral-vibe's middleware pattern.

```
internal/context/
  pipeline.go          — PromptPipeline, runs middlewares before each prompt
  middleware.go        — Middleware interface + MiddlewareAction enum
  project_context.go   — Builds workspace context (file tree, git status, OS info)
  agents_md.go         — Discovers and loads AGENTS.md files (walks up from workspace root)
  templates/           — Markdown prompt templates with $variable interpolation
    project_context.md
    agents_doc.md
  pipeline_test.go     — Tests
```

**Key types:**

```go
// MiddlewareAction controls what happens before a prompt is sent.
type MiddlewareAction int
const (
    ActionContinue       MiddlewareAction = iota
    ActionStop                            // reject the prompt
    ActionInjectMessage                   // prepend content to the prompt
)

// SessionContext provides session state to middlewares.
type SessionContext struct {
    SessionID    string
    WorkspaceID  string
    WorkspacePath string
    PromptCount  int  // 0 = first prompt
    UserPrompt   string
}

// Middleware runs before each prompt is sent to the agent.
type Middleware interface {
    BeforePrompt(ctx context.Context, sc *SessionContext) MiddlewareResult
    Reset()
}

// PromptPipeline chains middlewares and aggregates their results.
type PromptPipeline struct {
    middlewares []Middleware
}
```

**Built-in middlewares (Phase 1):**
- `FirstPromptContextMiddleware` — injects workspace context (file tree, git status, OS info) on first prompt only. Tracks `PromptCount` to know when to fire.
- `AgentsMdMiddleware` — injects `AGENTS.md` content from workspace root on first prompt. Uses the same priority hierarchy as mistral-vibe (closer files override distant ones).

**Future middlewares (not in Phase 1):**
- `SkillMiddleware` — inject skill content on `/skillname` commands
- `ContextWarningMiddleware` — warn when context is getting large
- `TurnLimitMiddleware` — enforce max turns per session

### Phase 2: Wire Into Prompt Flow

Modify `acp.Client.SendPrompt()` to run the pipeline before sending the prompt.

**Option A: Prepend to prompt text via pipeline (simple, universal)**

```go
func (c *Client) SendPrompt(ctx context.Context, sessionID, content string) error {
    session, ok := c.sessions[sessionID]
    // ...

    // Run middleware pipeline before sending the prompt
    if c.pipeline != nil {
        sc := &context.SessionContext{
            SessionID:     sessionID,
            WorkspaceID:   session.workspaceID,
            WorkspacePath: session.workspacePath,
            PromptCount:   session.promptCount,
            UserPrompt:    content,
        }
        result := c.pipeline.RunBeforePrompt(ctx, sc)
        if result.Action == context.ActionStop {
            return fmt.Errorf("prompt blocked: %s", result.Reason)
        }
        if result.Action == context.ActionInjectMessage && result.Message != "" {
            content = result.Message + "\n\n---\n\n" + content
        }
        session.promptCount++
    }

    // ... existing prompt flow (transport.Prompt)
}
```

- **Pros:** Works with every agent. No ACP protocol changes. Pipeline is extensible — new middlewares don't change the prompt flow. Each middleware decides independently whether to inject.
- **Cons:** Context counts against the user's token budget. Agent may treat it as user instruction rather than system context. If conversation is compacted, context may be lost.

**Option B: Use `NewSessionRequest.Meta` (cleaner, agent-dependent)**

```go
func (t *Transport) NewSession(ctx context.Context, cwd string, meta map[string]any) (string, error) {
    result, err := t.conn.NewSession(ctx, acp.NewSessionRequest{
        Cwd:        cwd,
        McpServers: []acp.McpServer{},
        Meta:       meta,
    })
    // ...
}
```

Pass `{"workspaceContext": "...", "fileTree": [...]}` in the Meta field.

- **Pros:** Clean separation. Doesn't pollute user messages. Agent can use it as system context.
- **Cons:** Agents are not required to read `_meta`. May be silently ignored. Unstable — ACP spec says "implementations MUST NOT make assumptions about values at these keys." Doesn't work with the middleware pattern (Meta is set once at session creation, not per-prompt).

**Option C: Use `PromptRequest.Prompt` with multiple ContentBlocks (spec-compliant)**

ACP's `PromptRequest.Prompt` accepts an array of `ContentBlock`. We could send a `ContentBlock::Resource` with the workspace context alongside the `ContentBlock::Text` user message. Confirmed: mistral-vibe's `_build_text_prompt()` handles `ContentBlock::Resource` by extracting `path` and `content` fields.

```go
Prompt: []acp.ContentBlock{
    acp.ResourceBlock(...),  // workspace context as resource
    acp.TextBlock(content),  // user message
}
```

- **Pros:** Spec-compliant way to attach context resources. Agent sees it as context, not user instruction. Works with mistral-vibe today.
- **Cons:** Requires checking `PromptCapabilities` to see if the agent supports resource blocks. Not all agents handle `Resource` blocks in prompts.

**Recommendation: Option A for Phase 1, Option C as a future enhancement.**

Option A is the most pragmatic — it works today with every agent, requires minimal code, and the pipeline architecture means we can swap to Option C later by changing how the pipeline result is packaged, without touching the middlewares themselves.

### Phase 3: Configuration

Expose context settings to the user via the UI settings panel and config file:

```json
{
  "contextProvider": {
    "enabled": true,
    "maxDepth": 3,
    "maxFiles": 200,
    "includeGitStatus": true
  }
}
```

- **Option A: Per-workspace settings** — Store in workspace registration. Different projects may want different depth.
- **Option B: Global daemon settings** — Single config in `~/.local-agent/config.json`. Simpler, applies to all workspaces.
- **Recommendation: Global settings with per-workspace override** — Start with global, add per-workspace later if needed.

### Phase 4: Refresh on Prompt (Optional Enhancement)

If the file tree changes significantly between prompts (files added/removed), we could re-inject an updated context. This is a tradeoff:

- **Pro:** Agent always knows current file layout.
- **Con:** Adds tokens to every prompt. May confuse the agent with duplicate context.
- **Recommendation: Skip for now.** First-prompt injection is sufficient. The agent can use terminal commands to check for new files if needed.

---

## Modularity

The middleware pipeline keeps each concern isolated and independently testable:

```
internal/context/
  pipeline.go          → PromptPipeline (chains middlewares, aggregates results)
  middleware.go        → Middleware interface, MiddlewareAction enum, SessionContext
  project_context.go   → FirstPromptContextMiddleware (file tree, git status, OS info)
  agents_md.go         → AgentsMdMiddleware (discovers + loads AGENTS.md)
  templates/           → Markdown prompt templates ($variable interpolation)
internal/acp/acp.go    → acp.Client gets optional *context.PromptPipeline field
internal/workspace/    → unchanged (already has FileTree)
```

- `internal/context` depends only on `interfaces.WorkspaceManager` and stdlib — no circular deps.
- `acp.Client` gets an optional `pipeline *context.PromptPipeline` field. If nil, behavior is unchanged (zero-config backward compatible).
- Each middleware is a standalone struct implementing one interface. Adding a new middleware doesn't touch existing ones or the pipeline.
- Templates are plain `.md` files with `$variable` placeholders — no code changes needed to tweak prompt wording.
- Config is a simple struct, easily serialized to JSON.
- Upgrading to Option C (resource blocks) later only changes how the pipeline result is packaged in `SendPrompt()`, not how middlewares produce content.
- The `PromptPipeline` is constructed in `internal/daemon/` where managers are wired together, keeping the `acp` package unaware of middleware specifics.

---

## Open Questions for Team Review

1. **First-prompt-only vs every prompt?** Recommend first-only. The agent retains it in conversation history. Re-injecting wastes tokens.

2. **File tree format — indented tree vs flat list?**
   - **Indented tree:** More readable, shows structure. Harder to parse programmatically.
   - **Flat list with paths:** `cmd/app/main.go`, `internal/acp/transport.go`, ... Easier for agent to use as reference.
   - **Recommendation:** Flat list with paths, grouped by top-level directory. Compact and directly useful.

3. **Git status — how much?**
   - Branch + clean/dirty summary (compact, ~1 line)
   - Branch + changed file list (useful, may be large)
   - Branch + changed files + recent commits (most context, most tokens)
   - **Recommendation:** Branch + summary count + last 5 commits. Keep it under ~500 chars.

4. **Should the context be visible in the UI chat?**
   - **Option A:** Show as a collapsed "Workspace context attached" banner above the first user message.
   - **Option B:** Invisible — injected server-side, not shown in chat.
   - **Recommendation:** Option A (collapsed banner). Transparency helps users understand what the agent knows.

5. **Should we use `AdditionalDirectories` to give agents access to parent directories?**
   - The `New_chat.json` bug showed the agent looking in `C:\Users\adama\OneDrive\Documents\aschool\` (parent of workspace). The agent was confused about its working directory.
   - `AdditionalDirectories` would let the agent read files from parent dirs without `cd` hacks.
   - **Risk:** Expands filesystem scope. Agent could read sensitive files outside workspace.
   - **Recommendation:** Don't use by default. Let users opt in per-workspace if needed.

6. **Windows path handling in context string?**
   - Should we show `C:\Users\adama\...` (native) or `/c/Users/adama/...` (Unix-style)?
   - **Recommendation:** Native paths. The agent should know it's on Windows (we can include OS info in the context, like mistral-vibe does).

---

## File Tree Size Budget

Based on mistral-vibe's `ProjectContextConfig` defaults:

| Parameter | mistral-vibe | Our Default | Rationale |
|---|---|---|---|
| max_depth | 3 | 3 | Enough to see package structure |
| max_files | 1000 | 200 | We're sending per-prompt, not system prompt |
| max_dirs_per_level | 20 | 20 | Same |
| max_chars | 40000 | 8000 | Per-prompt budget is smaller |
| timeout_seconds | 2.0 | 2.0 | Same |

A 200-file tree at ~40 chars/path = ~8KB. This is well within token budgets and provides enough structure for the agent to know where to look.

---

## Testing

- Unit test `PromptPipeline.RunBeforePrompt()`:
  - Empty middleware list → ActionContinue, no injection
  - Multiple middlewares → messages concatenated, first STOP halts
  - Reset clears state across all middlewares
- Unit test `FirstPromptContextMiddleware`:
  - First prompt (PromptCount=0) → ActionInjectMessage with context
  - Second prompt (PromptCount=1) → ActionContinue (no injection)
  - After Reset() → next prompt injects again
  - Empty workspace → minimal context (just path + OS)
  - Large workspace → truncation at maxFiles/maxChars
  - Non-git workspace → git status gracefully omitted
  - Deep nesting → depth limit enforced
- Unit test `AgentsMdMiddleware`:
  - AGENTS.md present in workspace root → content injected
  - AGENTS.md absent → ActionContinue, no injection
  - Multiple AGENTS.md files (nested dirs) → closest wins
- Integration test: verify context appears in the first prompt sent to an agent (mock transport)
- Manual test: ask agent "Summarize readme.md" and verify it can find the file without shell commands
