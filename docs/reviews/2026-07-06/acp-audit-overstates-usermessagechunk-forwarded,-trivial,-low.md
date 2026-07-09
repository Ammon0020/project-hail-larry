# user_message_chunk handling overstated as "forwarded to UI"

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `docs/reviews/2026-07-06/acp-audit.md`
- **Lines:** 20

## Description

Audit item 4 states: "`SessionUpdate` switches on all update types ... **Each is translated to the internal Event system and forwarded to the UI.**" This is inaccurate for `user_message_chunk`: the corresponding case in `internal/acp/transport.go:105-106` is a no-op with only a comment ("Usually already emitted by our side when the prompt was submitted.") — no event is emitted and nothing is forwarded. The switch *recognizes* all six types, but only five produce events. This matters because the audit is a compliance document; overstating coverage could lead a reviewer to believe user-message echo is handled when it is intentionally suppressed.

## Recommendation

Reword to "Each update type is recognized; five of the six are translated to internal events and forwarded to the UI (`user_message_chunk` is intentionally suppressed since the client already emits the user's own prompt)."

## Verification

Read `internal/acp/transport.go:34-108` — the `UserMessageChunk` case (lines 105-106) contains only a comment and no `OnEvent` call, unlike the other five cases.
