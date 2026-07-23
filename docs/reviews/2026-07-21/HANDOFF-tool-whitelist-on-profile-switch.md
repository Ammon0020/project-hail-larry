# Handoff: Live profile switches do not apply the tool whitelist

- **Difficulty:** hard
- **Urgency:** high
- **Source finding:** [profile-switch-does-not-apply-tool-whitelist,hard,high.md](profile-switch-does-not-apply-tool-whitelist,hard,high.md)
- **Status:** deferred to external developer

## The problem

When a user switches profiles on a live session via `POST /sessions/:id/profile`,
the backend correctly:

1. Stores the new profile id in `ProfileMiddleware` (`src/acp/profile.rs`).
2. Sends `session/set_config_option` (mode category) to the agent over ACP
   (`src/acp/providers.rs::rpc_set_profile_config`).

But it does NOT update the MCP tool boundary. The per-profile tool whitelist
(configured in `~/.local-agent/profiles.json` as `tools: string[]` per profile)
is only applied at session setup, in `load_session_mcp_servers`
(`src/acp/core.rs:1540`), which calls `tools_for_session(session_id)`
(`src/mcp/tools.rs`) to filter each MCP server's tool list. After a live
profile switch, the session keeps its original tool set — a session created
under a permissive profile retains those tools after switching to a
restrictive one.

## Why it's hard

Two viable approaches, each with trade-offs:

### Option A: Rebind MCP servers on profile switch

Re-run `load_session_mcp_servers` with the new profile's whitelist and
re-attach the filtered MCP servers to the live actor. This requires either:

- A new `ActorCommand::RebindMcp { servers, result }` variant that swaps the
  actor's MCP connection set at runtime, OR
- A full session rebind (teardown + recreate) which is heavier and loses
  agent-side conversation state.

ACP v1 does not define a live "replace MCP servers" RPC — the MCP server list
is fixed at `session/new` / `session/load` time. So this approach requires
either a client-side transport swap (the client owns the MCP connections per
the architecture in `AGENTS.md`) or a session rebind.

**Pros:** Clean — the agent sees exactly the tools it's allowed to call.
**Cons:** Non-trivial actor surgery; ACP has no native support for this.

### Option B: Enforce the whitelist at tool invocation time

Keep the MCP servers attached as-is, but filter tool calls at the permission
layer. When the agent sends a `tool_call` request, check the active profile's
whitelist before forwarding to the MCP server. Reject with a clear error if
the tool isn't in the whitelist.

This is a check in the tool-call dispatch path (likely in
`src/acp/core.rs` where `tool_call` requests are handled — search for
`tool_call` or `ToolCallRequest` in the actor loop).

**Pros:** Simpler — no transport surgery; the whitelist is enforced at the
boundary that matters (actual tool execution).
**Cons:** The agent still *sees* all tools in its `tools/list` response and
may attempt calls that get rejected, wasting a round-trip. The UX is worse
than Option A but the security boundary is intact.

### Recommendation

**Option B is the pragmatic v1 fix** — it's a fraction of the code, enforces
the security boundary correctly, and doesn't require ACP protocol changes.
Option A can be a follow-up if the UX of rejected tool calls is too rough.

## Key files

| File | Role |
|------|------|
| `src/acp/core.rs:1540` | `load_session_mcp_servers` — where the whitelist is currently applied (session setup only) |
| `src/acp/core.rs:1067-1087` | `set_session_profile` — the live switch path that needs the fix |
| `src/mcp/tools.rs` | `tools_for_session(session_id)` — returns the filtered tool list for the active profile |
| `src/acp/profile.rs` | `ProfileMiddleware` — stores the active profile per session; `profile(session_id)` reads it |
| `src/acp/core.rs` (actor loop) | Where `tool_call` requests are dispatched to MCP servers — the enforcement point for Option B |

## How to verify the fix

1. Create a session under a permissive profile (e.g. `code` with all tools).
2. Switch to a restrictive profile (e.g. a custom profile with `tools: ["read_file"]`).
3. Have the agent attempt a `tool_call` for a tool NOT in the whitelist
   (e.g. `write_file`).
4. Assert the call is rejected (Option B) or not offered (Option A).
5. Assert a whitelisted tool call still succeeds.

The existing test
`load_session_mcp_servers_respects_profile_tool_whitelist` (`src/acp/core.rs:3472`)
covers the session-setup path — use it as a pattern reference.

## Project conventions

- Read `AGENTS.md` for coding, testing, and security conventions.
- Run `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D warnings`,
  `cargo fmt --check -q`, and `make test-contract` before finishing.
- Use `tracing` for logging. Brief comments for non-obvious intent.
- Fail loudly — don't suppress errors to make tests pass.
