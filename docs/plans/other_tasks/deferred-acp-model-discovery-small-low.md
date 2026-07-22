# Task: Replace CLI/file model detection with a single ACP probe

> **Status:** deferred | **Urgency:** low | **Difficulty:** small
> **Scope:** `src/acp/autodetect/`

## Context

Per-harness model probes live under `src/acp/autodetect/{cursor,devin,...}.rs`.
They still use non-standard paths because ACP has no stable pre-auth model list:

- **Cursor**: `agent --list-models` (CLI must be authed).
- **Devin**: ACP `authenticate` (local `_meta.api_key` or timed browser PKCE)
  then `session/new` → `configOptions` model selector.
- **Codex / Vibe**: local cache/config files.

`providers/list` still returns `-32601` on Devin/Cursor (verified 2026-07-18).

## The proper path (when ACP catches up)

A single ACP `models/list` (or stable `providers/list`) replaces all harness-
specific `detect_models` bodies. Keep the modular harness files; only their
probe implementation changes.

## Trigger to unblock

- ACP publishes a stable model-listing method, OR
- Agents implement `providers/list` without auth.
