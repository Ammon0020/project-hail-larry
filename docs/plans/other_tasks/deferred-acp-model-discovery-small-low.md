# Task: Replace config-file/CLI-probe model detection with ACP session probe

> **Status:** deferred | **Urgency:** low | **Difficulty:** small
> **Scope:** `src/acp/autodetect.rs`

## Context

Model autodetection for Cursor and Devin currently uses workarounds because
ACP does not yet expose a standard, pre-auth model-listing method:

- **Cursor**: `ModelSource::CursorConfig` reads `~/.cursor/cli-config.json`
  for the account's current model, after trying `agent --list-models` (which
  only works when the account is authed).
- **Devin**: `ModelSource::None` with a hardcoded fallback list matching
  `devin --help`'s `--model` examples.

Both agents return `Method not found` for the unstable `providers/list` ACP
method, and `session/new` (whose response includes `sessionConfig.options`
with a `model` selector) requires authentication first.

## The proper path (when ACP catches up)

1. ACP standardizes a pre-auth model listing method (e.g. `providers/list`
   becomes stable, or a new `models/list` is added), OR
2. The daemon authenticates during autodetect and calls `session/new` to read
   `sessionConfig.options` model selector options.

When either happens, `CursorConfig`, `CursorCli`, `CodexCache`, `VibeConfig`,
and the fallback model lists can all be replaced by a single ACP probe path.

## Trigger to unblock

- ACP spec publishes a stable model-listing method, OR
- Devin and Cursor both implement `providers/list` (currently both return
  `-32601 Method not found`).

## Out of scope

- Adding auth to autodetect today (too much complexity for a startup probe).
- Per-agent integrations (violates the ACP-only architecture rule).
