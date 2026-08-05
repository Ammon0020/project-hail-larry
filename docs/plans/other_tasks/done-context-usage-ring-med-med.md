# Context window usage ring and cost display

> **Status:** done | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — Chat section

## Goal

Display real-time context token usage and cost metrics using ACP's
`session/update` with `sessionUpdate: "usage_update"`.

## Behavior

1. **Ring around send button**: fills clockwise as context fills, up to 100%
   of the model's context window. When compacting, an inner ring spins to
   indicate compaction in progress.

2. **Hover/tap popout**:
   - "<x>% (<y>k/<z>k) context used"
   - Cost
   - Prompt cache expiry timer: "Prompt cache expires in x" counting down to
     the second. If cache time unknown for the model, show "Estimated: Prompt
     cache expires in x". Text turns orange when expired.
   - On mobile: shows on tap for a few seconds or on long-press.

## Dependencies

- Agent must advertise `usage_update` in session notifications
- Model context window size needed for percentage calculation

## Acceptance

- [x] Context ring around send button fills as context grows
- [ ] Inner ring spins during compaction — **deferred**: ACP `usage_update`
      does not carry a compaction signal. See "Deferred items" below.
- [x] Hover/tap popout shows percentage, token count, and cost
- [ ] Prompt cache expiry countdown timer — **deferred**: ACP `usage_update`
      does not carry cache expiry. See "Deferred items" below.
- [ ] Orange text when cache expired — **deferred** (depends on cache expiry).
- [x] Mobile-friendly tap interaction (click/tap toggles popout; hover on desktop)
- [x] `make check` passes (Rust fmt/clippy/tests, frontend lint/build/tests, contracts)

## Deferred items

Two acceptance items are blocked on the ACP protocol, not on this codebase:

1. **Compaction signal**: The stabilized ACP `usage_update` (schema v1.5.0)
   carries only `used`, `size`, `cost`, and `_meta`. There is no compaction
   flag. An inner spinner cannot be driven by protocol data. Revisit if a
   future ACP version adds a compaction notification, or infer heuristically
   from a sudden drop in `used` (unreliable — deferred).

2. **Prompt cache expiry**: ACP `usage_update` does not include cache expiry.
   The `_meta` field is reserved for extensibility but agents must not be
   assumed to populate it. A fixed timer per model would be unreliable.
   Revisit if ACP adds cache expiry to the stabilized schema.

Both are recorded here so a future story can pick them up when the protocol
catches up. The ring + cost display (the protocol-supported subset) is complete.

## Implementation notes

- **Backend**: New `EventType::UsageUpdated` + `EventPayload::UsageUpdated`
  variant (30 event types, up from 29). `src/acp/stream.rs` maps ACP
  `UsageUpdate` → typed payload. Four new optional fields on the flat `Event`
  wire struct: `tokensUsed`, `tokensSize`, `costAmount`, `costCurrency`
  (all `omitempty`, so existing golden fixtures are unaffected).
- **Frontend**: `ContextUsageRing` wraps the send button in `ChatComposer`,
  rendering an SVG ring that fills clockwise. `ChatPanel` derives the latest
  `UsageUpdated` event from the session's event stream and passes it down.
  Pure math helpers in `web/src/lib/contextUsage.ts` (10 vitest cases).
- **Ring visibility**: Hidden until the first `UsageUpdated` event arrives —
  agents that don't report usage won't show a misleading 0% ring. Color
  shifts muted → primary → destructive at 50% / 90% fill.
