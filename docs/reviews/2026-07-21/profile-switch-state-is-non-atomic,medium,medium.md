# Profile state can diverge from the ACP agent

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `/media/adam/extex/projects/project-hail-larry/src/acp/core.rs`
- **Lines:** 1059-1087

## Description

The method mutates `ProfileMiddleware` before enqueueing and awaiting the ACP update, without serializing the local mutation with the actor command. If `sender.send`, the response channel, or `rpc_set_profile_config` fails, the endpoint returns an error even though the local profile has already changed; the next prompt is prepared with that new profile while the agent may still have the old mode. Concurrent successful calls can also diverge: call A can store `ask`, call B can store and enqueue `plan`, then A can enqueue `ask`, leaving local state at `plan` but the actor at `ask`. A concurrent close can similarly pass the existence check and then leave an orphaned profile entry while returning success.

## Recommendation

Serialize the complete per-session profile transition and define a single commit point. Prefer having the actor apply the ACP option and update the local profile in command order, with explicit behavior for capability-less/dormant sessions. On RPC failure, either roll back the local value or deliberately return success as a documented fallback; do not return an error after silently committing only half of the transition. Revalidate session existence at commit time.

## Verification

Lines 1059-1065 perform an unlocked existence check, line 1069 immediately writes shared profile state, and only afterward do lines 1074-1087 enqueue and await the actor RPC. There is no per-session mutex spanning those operations, and `handle_non_prompt_command` performs only the remote RPC for `SetProfile` at lines 1848-1855, so actor ordering cannot order or roll back the earlier local writes.
