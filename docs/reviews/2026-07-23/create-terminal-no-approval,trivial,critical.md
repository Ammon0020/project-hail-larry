# create_terminal executes agent commands with no permission gate (approval bypass)

- **Difficulty:** trivial
- **Urgency:** critical
- **File:** `src/acp/core.rs`
- **Lines:** 2347-2417 (handler), 1470-1484 (registration)

## Description

`request_permission` (line 2637) and `create_terminal` (line 2347) are independent ACP request handlers with **no binding between them**. `request_permission` only records a decision in the `PermissionManager` and returns it to the agent; it does not execute anything and does not stamp an "approved command" that later execution must match. `create_terminal` spawns the command immediately via `Executor::run_async_args` and never consults `deps.permissions` (the field exists in `HandlerDeps` at line 2186 but is unused in `create_terminal`). The daemon advertises `terminal(true)` capability to the agent (line 1560), so any agent can call `create_terminal` directly. A malicious or prompt-injected agent can simply skip `session/request_permission` and call `session/create_terminal` with `command="rm", args=["-rf", "/"]` (or `command="sh", args=["-c", "..."]`) — no prompt is ever shown to any paired device. This defeats the entire approval threat model stated in AGENTS.md ("Agents propose actions; the client performs approved actions"). The permission system is advisory, not enforced.

## Recommendation

Gate `create_terminal` on a prior approval. Either (a) require the agent to pass the `request_permission` id and bind the approved `tool_call.raw_input` / command / args / cwd / env to the terminal creation, rejecting any mismatch (TOCTOU-safe binding under the manager lock), or (b) have `create_terminal` itself call `deps.permissions.request(...)` with the actual command/args/cwd/env before spawning. Option (b) is simpler and removes the unbound two-step entirely. The approved action must be bound to the exact argv/cwd/env that is executed, not just a display string.

## Verification

`create_terminal` (lines 2347-2417) contains no call to `deps.permissions` or any `PermissionManager` method. The handler is registered at lines 1470-1484 with `spawn_result_callback(... create_terminal(deps, request))` — direct execution. `request_permission` (line 2695) calls `deps.permissions.request(permission).await` but its return value only shapes the ACP response; nothing persists an "approved command" that `create_terminal` checks. `HandlerDeps.permissions` (line 2186) is plumbed but unused by `create_terminal`.
