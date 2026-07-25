# Missing authorization on cancel-revocation / cancel-workspace-registration

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/api/mod.rs`
- **Lines:** 412-424 (cancel_revocation), 1494-1511 (cancel_pending_action); `src/pairing/mod.rs:502-518`

## Description

`cancel_revocation` and `cancel_workspace_registration` are protected only by `require_auth` (caller is *some* paired device). `cancel_pending_action` decodes `actionId` and calls `pairing.cancel_revocation(id)` with **no check** that the caller is not the device being revoked, is not the original requester relationship, etc. `pairing::cancel_pending` only verifies the action_type matches. Consequently, a device that is the target of a pending revocation can call `POST /api/devices/cancel-revocation` with its own action id and permanently prevent its own revocation, defeating the grace-period revocation feature entirely. The same flaw lets any paired device cancel any other device's pending workspace registration.

## Recommendation

In `cancel_pending_action`, resolve the caller's `device_id` (via `device_id_from_request`) and reject when `action.info.action_type == REVOCATION && action.info.device_id == caller_device_id`. Consider also requiring the caller to be a *different* paired device or the host for revocation cancellation, and record `requested_by` on the cancel event for audit.

## Verification

`cancel_pending_action` (`api/mod.rs:1494-1511`) calls `cancel(&request.action_id)` with no caller identity argument. `pairing::cancel_pending` (`pairing/mod.rs:507-517`) checks only `action.info.action_type != action_type`. `device_id_from_request` is computed in `revoke_device` (`api/mod.rs:384`) for the *event* but never used for authorization. The test `pending_revocation_can_be_cancelled_or_executes` (`pairing/mod.rs:792-806`) cancels with no caller credential at all.
