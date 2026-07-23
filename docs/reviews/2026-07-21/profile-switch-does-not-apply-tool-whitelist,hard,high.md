# Live profile switches do not apply the selected tool whitelist

- **Difficulty:** hard
- **Urgency:** high
- **File:** `/media/adam/extex/projects/project-hail-larry/src/acp/core.rs`
- **Lines:** 1067-1069

## Description

`set_session_profile` stores the new profile and claims the tool whitelist will see it on the next prompt, but a live actor's MCP servers were already fixed during session setup. The only `tools_for_session` read occurs through `load_session_mcp_servers` while constructing `session/new`/`session/load` (lines 1540-1546 and 2496-2544). No profile-switch path updates or recreates that MCP attachment. Consequently, a session created under a permissive/default profile keeps those tools after switching to a restrictive profile, so the configured per-profile whitelist is not enforced for the normal live-switch flow.

## Recommendation

Make profile changes update the effective tool boundary. Depending on ACP support, either cleanly rebind/recreate the session with MCP servers filtered for the new profile, or enforce the active profile's whitelist at tool invocation so it can change without rebuilding transport state. Do not report the profile switch as complete until both the ACP mode and tool policy agree.

## Verification

The new switch path only calls `ProfileMiddleware::set_profile` and optionally queues `ActorCommand::SetProfile` (lines 1067-1087). A repository search found `tools_for_session` used only by `load_session_mcp_servers`, which is called at actor startup at lines 1540-1546; there is no corresponding whitelist refresh in `SetProfile` handling.
