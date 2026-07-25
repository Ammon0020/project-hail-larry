# Unbounded ACP session creation enables process-exhaustion DoS

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/acp/core.rs`
- **Lines:** 205-284 (create_session_with_profile), 462-517 (register_live_session)

## Description

`create_session_with_profile` and `register_live_session` spawn one `tokio::spawn(run_actor(...))` per session, each of which spawns a child agent process (`Command::new(...).spawn()` at line 1364). There is no semaphore, no session-count cap, and no check against a `MAX_SESSIONS` constant anywhere in the ACP crate. The only `Semaphore` in the crate is `callback_slots` (line 1396, per-session, capacity `MAX_CALLBACK_TASKS = 16`). A paired device (or any caller that can reach `POST /api/sessions`) can create an unbounded number of sessions, each pinning an agent process and its process group. This is a host resource-exhaustion vector.

## Recommendation

Add a global `Semaphore` (or a simple live-count check in `register_live_session`) capping concurrent live sessions. Reject `create_session` with a 503/429 when the cap is reached. The cap should be configurable.

## Verification

`register_live_session` (line 462) resolves the agent + workspace, builds `ActorConfig`, calls `tokio::spawn(run_actor(...))` (line 495), and inserts into `self.sessions` (line 514) with no count check. `create_session_with_profile` (line 205) calls it directly. `grep -n 'Semaphore|MAX_SESSIONS|session_limit' src/acp/` returns only `MAX_CALLBACK_TASKS` and `MAX_TERMINALS_PER_SESSION`, both per-session.
