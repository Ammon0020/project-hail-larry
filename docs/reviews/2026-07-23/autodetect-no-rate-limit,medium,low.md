# autodetect_agents not rate-limited beyond body limit — probe spawn DoS

- **Difficulty:** medium
- **Urgency:** low
- **File:** `src/api/mod.rs`
- **Lines:** 991-993

## Description

`autodetect_agents` calls `acp::autodetect().await` which spawns probe processes for each known harness (up to 5: claude_code, codex, cursor, devin, vibe). Each probe has an 8-second timeout (`src/acp/autodetect/mod.rs:79`). The endpoint is in the `protected` router (requires auth) but has no per-caller rate limit. A paired device can repeatedly call `POST /api/agents/autodetect` to spawn up to 5 child processes per request, consuming host resources. The commands are hardcoded (not user-supplied), so this is a DoS vector only, not RCE.

## Recommendation

Add a per-device or global cooldown (e.g., reuse the `pair_rate` token bucket pattern) or cache results for a short TTL.

## Verification

Line 991-993: no rate limiting. `autodetect()` at `src/acp/autodetect/mod.rs:96` iterates all harnesses. `detect_models_for` at line 120 may spawn probes via `probe_providers` (`src/acp/autodetect/common.rs:128`: `Command::new(command).spawn()`). The route is at line 179: `.route("/api/agents/autodetect", post(autodetect_agents))` — only `require_auth` applies, no `require_pair_rate_limit`.
