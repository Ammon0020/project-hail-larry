# Permission prompt timeout/expiry

> **Status:** done | **Difficulty:** medium | **Urgency:** high
> **Source:** chat reliability audit

## Problem

Unanswered permission prompts block the agent indefinitely. The stale
sweeper (`src/permissions/manager.rs:206-210`) only clears permissions
for expired sessions, not unanswered ones. If no device is available to
approve, the agent hangs forever in `Running` state.

## Goal

Add a configurable timeout for pending permission prompts. When the
timeout fires, the permission is auto-denied and the agent receives a
rejection, allowing it to proceed or fail gracefully.

## Behavior

1. When a permission request is created, a timeout timer starts
   (default 300s / 5 min, configurable).
2. If no device answers within the timeout, the permission is
   auto-denied with reason "Permission timed out — no device available".
3. The agent receives the denial via the normal permission response
   channel.
4. The frontend shows a warning in the chat: "Permission request timed
   out (no device available to approve)".
5. The timeout is visible in the permission prompt UI as a countdown.

## Dependencies

- Configurable timeout in `config.toml`
- Permission manager timer integration

## Acceptance

- [x] Unanswered permissions auto-deny after timeout
- [x] Agent receives denial and can proceed/fail
- [x] Frontend shows timeout warning in chat
- [x] Timeout configurable via `config.toml`
- [x] Tests: timeout triggers auto-deny
- [x] `make check` passes
