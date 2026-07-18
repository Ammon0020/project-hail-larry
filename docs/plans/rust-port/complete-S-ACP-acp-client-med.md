# ACP Work Breakdown

> **Status:** Superseded as an implementation story by the split ACP work items.
> **Go source:** `internal/acp/` (5,066 lines)

## Purpose

`internal/acp/` remains the source inventory for the Rust port, but it is too
large and cross-cutting to implement or review as one story. Its responsibilities
are now split so SDK risk, process ownership, wire-event translation, and
ancillary features can be independently verified.

## Successor Stories

1. [S-ACP-SPIKE](S-ACP-SPIKE-sdk-proof.md) — verify current SDK capability
2. [S-ACP-CORE](S-ACP-CORE-session-transport.md) — lifecycle and handlers
3. [S-ACP-STREAM](S-ACP-STREAM-events.md) — notifications to durable events
4. [S-ACP-CONTEXT](S-ACP-CONTEXT-conversation.md) — context and conversation
5. [S-ACP-PROVIDERS](S-ACP-PROVIDERS-providers.md) — provider capability
6. [S-ACP-AUTODETECT](S-ACP-AUTODETECT-registry.md) — registry and probing

## Non-Negotiable Constraints

- Use APIs proven by S-ACP-SPIKE rather than stale SDK examples.
- Preserve existing external behavior through S-CONTRACT.
- One session owns its cancellation token, child process tree, and tasks.
- No session-map or service lock is held across `.await`.
- Persist app events before publishing them to WebSocket subscribers.
