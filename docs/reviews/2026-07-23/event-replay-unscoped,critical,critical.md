# Event replay is unscoped — any paired device receives every session's private events

- **Difficulty:** trivial
- **Urgency:** critical
- **File:** `src/sync/mod.rs`
- **Lines:** 560-581 (resync_replay_only), 425-447 (feed_task), 269-278 (WsQuery); `src/events/store.rs:180-204`; `src/api/mod.rs:175,892-902`

## Description

When a device reconnects with `?after=<id>`, `resync_replay_only` calls `bus.query_all(after_id, 0)`, which executes `SELECT id, type, session_id, timestamp, payload FROM events WHERE id > ?1 ORDER BY id ASC LIMIT ?2` — across **all** sessions, with no filter by `session_id`, `workspace_id`, or `device_id`. The `WsQuery` struct accepts only `device_id`, `secret`, and `after`; there is no session/workspace parameter, so clients cannot even request scoping. The `after` cursor is client-controlled and defaults to `0`, so a newly paired device can pass `?after=0` and receive the **entire historical event log**.

Event payloads contain `content` (prompts/responses), `command` + `cwd` (shell commands), `target` + `summary` (file edits), and `attachments` — i.e., other devices' private shell output, file contents, and conversation history. The same unscoped `query_all` is exposed via the authenticated HTTP endpoint `GET /api/events`, compounding the issue.

This is a cross-device information disclosure: any single paired device (or any process on loopback, which bypasses auth entirely) can read every other device's past sessions.

## Recommendation

Filter replay by the authenticated device's authorized sessions/workspaces. At minimum, add a `session_id` (or `workspace_id`) parameter to `WsQuery` and validate it against the device's ownership before replay; use `Store::query` (session-scoped) instead of `query_all` for WS replay. For multi-session devices, maintain a per-device session allowlist and intersect replay results against it. Do not allow `after=0` full-history replays from newly paired devices without an explicit admin grant. Apply the same scoping to `GET /api/events`.

## Verification

`resync_replay_only` (`src/sync/mod.rs:566-569`) calls `bus.query_all(after_id, 0)`. `query_all_blocking` (`src/events/store.rs:188-192`) runs `WHERE id > ?1` with no session filter. `WsQuery` (`src/sync/mod.rs:271-278`) has no session field. `run_client_pumps` (`src/sync/mod.rs:425-447`) passes the raw client `after` cursor straight to `resync_replay_only`. `Event::session_id` and `StoredEventPayload::workspace_id` exist but are never used as replay filters.
