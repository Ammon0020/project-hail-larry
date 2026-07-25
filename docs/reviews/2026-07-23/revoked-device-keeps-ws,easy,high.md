# Revoked device retains active WebSocket session

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/sync/mod.rs`
- **Lines:** 352-397 (handle_ws), 287-310 (authorize_handshake); `src/pairing/mod.rs:287-298,485-489`

## Description

WebSocket authentication runs **only at handshake time**. `handle_ws` calls `authorize_handshake` (which invokes the `AuthChecker` → `Manager::validate_credential`) once before upgrade; `run_client_pumps` never re-validates. When `revoke_device`/`execute_pending` removes a device from `inner.devices`, no signal is sent to the hub to drop that device's existing connection. A revoked device therefore continues to receive every broadcast event (file changes, other devices' permission prompts, future pairing/revocation events) until it voluntarily disconnects or the hub shuts down. This defeats the intent of revocation for an active attacker who simply holds the socket open.

## Recommendation

Track the `device_id` on each `ClientEntry` (or a `HashSet<device_id>` of revoked devices on the hub) and have `revoke_device`/`execute_pending` call into the hub to force-close connections whose stored device_id matches. Re-checking `validate_credential` on each reconnect (`?after=`) is not sufficient because the socket stays open between reconnects.

## Verification

`handle_ws` (`sync/mod.rs:380-390`) gates only the upgrade; `run_client_pumps` (`sync/mod.rs:400+`) has no auth re-check. `revoke_device` (`pairing/mod.rs:287-298`) and `execute_pending` (`pairing/mod.rs:485-489`) mutate `inner.devices` and `save_devices` but never touch the hub. `Hub` exposes no "drop by device_id" API (only `shutdown`/`unregister` by internal u64 id).
