# Cancel is cooperative — malicious agent can ignore cancel and keep process alive

- **Difficulty:** medium
- **Urgency:** low
- **File:** `src/acp/core.rs`
- **Lines:** 1016-1030 (cancel_session), 2133-2139 (actor Cancel handling), 2235-2246 (spawn_callback)

## Description

`cancel_session` sets a sticky bit and sends `ActorCommand::Cancel`. The actor loop handles `Cancel` by calling `send_cancel` — a JSON-RPC *notification* to the agent (line 2176) — and then returns `PromptExit::Continue` without killing the child. The agent process is only reaped on `Close` (process-group kill at lines 1700-1706). In-flight callback tasks (`spawn_callback`, line 2235) are tied to `handler_cancel` (a `CancellationToken`), but that token is only cancelled when the SDK connection drops — `Cancel` does not fire it. So a malicious agent that ignores the `Cancel` notification continues to receive prompt results, can keep issuing `read_text_file`/`write_text_file`/`create_terminal` callbacks, and its process survives until the user explicitly closes the session.

## Recommendation

On `Cancel`, also fire `handler_cancel` so in-flight callbacks are aborted, and impose a timeout after which the agent process is force-killed if it has not acknowledged the cancel. Consider making `cancel_session` escalate to `close_session` after a grace period.

## Verification

`cancel_session` (line 1016) calls `prompt_cancel.store(true, ...)` and sends `ActorCommand::Cancel`, then `update_state(... Interrupted)`. The actor `Cancel` arm (line 2133) calls `send_cancel` (a notification, line 2176) and returns `Ok(PromptExit::Continue)`. `handler_cancel` (`HandlerDeps::cancellation`, line 1383/2189) is never cancelled by the `Cancel` command path — it is only cancelled implicitly when the SDK connection ends. `spawn_callback` (line 2235) selects on `cancellation.cancelled()` and the future, so callbacks continue running until the connection drops.
