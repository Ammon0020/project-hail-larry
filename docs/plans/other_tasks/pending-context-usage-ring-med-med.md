# Context window usage ring and cost display

> **Status:** pending | **Difficulty:** medium | **Urgency:** medium
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

- [ ] Context ring around send button fills as context grows
- [ ] Inner ring spins during compaction
- [ ] Hover/tap popout shows percentage, token count, and cost
- [ ] Prompt cache expiry countdown timer
- [ ] Orange text when cache expired
- [ ] Mobile-friendly tap interaction
- [ ] `make check` passes
