# Agent Context Pipeline

> Design reference for the prompt middleware pipeline in `internal/acp/`.
> Referenced by `context.go` line 9.

## Overview

Agents receive no workspace context on their first prompt, which forces them to
discover files via shell round-trips. The prompt middleware pipeline injects a
compact context bundle into prompts so the agent can start working immediately
and stays aware of editor state as it changes.

The pipeline is wired in `internal/daemon/daemon.go` and runs inside
`SendPrompt` (`internal/acp/acp.go`) before each prompt is sent to the agent.
Injected context is prepended to the user's prompt content as text (the
existing pattern). Switching to ACP `ContentBlock::Resource` embedded context
is a separate future task — the text-based approach works with all agents.

## Architecture

```
SendPrompt
  └─ PromptPipeline.RunBeforePrompt(ctx, pc)
       ├─ FirstPromptContextMiddleware   (first prompt only)
       │     workspace root, platform, file tree, git status, AGENTS.md
       ├─ TimeMiddleware                 (every prompt)
       │     current time (ISO 8601 with timezone)
       ├─ OpenFilesMiddleware            (every prompt)
       │     currently open file paths (from frontend)
       └─ RecentEditsMiddleware          (every prompt)
             recently edited file paths (from frontend)
```

Each middleware implements `PromptMiddleware`:

```go
type PromptMiddleware interface {
    BeforePrompt(ctx context.Context, pc *PromptContext) (PromptAction, string)
}
```

The pipeline concatenates injected sections with a `\n\n---\n\n` separator and
tracks a per-session prompt counter so middlewares can distinguish the first
prompt (`PromptCount == 0`) from subsequent ones.

## Externalized Templates

Header strings and numeric limits are externalized to
`configs/system-messages.json` so operators can customize the injected wording
and caps without editing Go source. The `SystemMessages` struct
(`internal/acp/messages.go`) mirrors the JSON. `LoadSystemMessages` reads the
file and falls back to `DefaultSystemMessages()` (matching the original
hardcoded values) when the file is missing or unreadable.

Header strings support `{placeholder}` substitution via `SystemMessages.Render`,
e.g. `"## Files (first {count}, depth ≤ {depth})"`.

## Frontend Integration

The frontend reports its editor state (open files, recent edits) to the backend
via `POST /api/sessions/{id}/context`:

```json
{ "openFiles": ["src/a.go", "src/b.go"], "recentEdits": ["src/c.go"] }
```

The endpoint updates an in-memory `OpenFilesTracker` (`internal/acp/messages.go`)
which the `OpenFilesMiddleware`/`RecentEditsMiddleware` consult before each
prompt. The tracker starts empty; middlewares skip injection (no empty
sections) until the frontend reports state. Both fields are optional — omitted
fields leave the tracker unchanged.

## Files

- `internal/acp/context.go` — pipeline core + `FirstPromptContextMiddleware`
- `internal/acp/messages.go` — `SystemMessages`, loader, `OpenFilesTracker`
- `internal/acp/providers.go` — `TimeMiddleware`, `OpenFilesMiddleware`,
  `RecentEditsMiddleware`, `OpenFilesProvider` interface
- `configs/system-messages.json` — customizable templates and limits
- `internal/daemon/daemon.go` — wires the pipeline + tracker
- `internal/server/api.go` — `POST /api/sessions/{id}/context` handler
