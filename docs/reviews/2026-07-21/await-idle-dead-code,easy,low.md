# await_idle helper is dead code — send_prompt already guarantees idle state

- **Difficulty:** easy
- **Urgency:** low
- **File:** `tests/acp_core_lifecycle.rs`
- **Lines:** 152-166 (helper), 191, 217 (call sites)

## Description

The `await_idle` helper polls `client.get_session_info(session_id)` every
25ms until `info.status == "idle"`, with a 15s timeout. It is called after
`client.send_prompt(...).await.expect("prompt")` in two tests
(`mockagent_initial_profile_sent_over_acp_when_capability_advertised` and
`mockagent_set_session_profile_switches_over_acp`).

However, `send_prompt` (src/acp/core.rs:665-733) already awaits the actor
result and then calls
`self.update_state_if(session_id, SessionState::Running, SessionState::Idle)`
(line 732) before returning `Ok(())`. When `send_prompt` returns `Ok`,
the session state is **already** `Idle` in the sessions map. Since
`get_session_info` reads from the same map under the same lock, the first
poll in `await_idle` always sees `Idle` and returns immediately.

If `send_prompt` returns `Err`, the test's `.expect("prompt")` panics
before `await_idle` is reached. So `await_idle` is unreachable in any path
where it would actually need to wait.

This helper is also the sole reason for the new
`#![allow(clippy::panic)]` attribute added at line 5 of the file —
`await_idle` uses `panic!()` for its timeout. Removing the helper would
allow tightening the lint.

## Recommendation

Remove the `await_idle` helper (lines 152-166) and its two call sites
(lines 191 and 217). Then remove `clippy::panic` from the
`#![allow(...)]` attribute on line 5 (keeping `clippy::expect_used` and
`clippy::unwrap_used`).

If there is a concern about event-store flush latency (events appended
asynchronously after `send_prompt` returns), replace `await_idle` with a
small bounded sleep or an event-store query retry loop that waits until
the expected `StreamUpdate` events appear — but the current `await_idle`
does not solve that problem either, since it checks session status, not
event availability.

## Verification

- Read `src/acp/core.rs:726-733`: `send_prompt` calls
  `update_state_if(..., Running, Idle)` then returns `Ok(())` — state is
  Idle before the caller sees `Ok`.
- Read `src/acp/core.rs:603-615`: `get_session_info` reads the same
  sessions map that `update_state_if` writes to (line 527), so the Idle
  state is visible immediately.
- If `send_prompt` returns `Err`, `.expect("prompt")` panics before
  `await_idle` is called — confirmed by reading the test bodies at lines
  188-191 and 214-217.
