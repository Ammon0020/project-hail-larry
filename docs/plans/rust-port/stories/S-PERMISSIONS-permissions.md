# Story S-PERMISSIONS: Permission Manager

> **Phase:** 2 | **Depends on:** S-EVENTS | **Go source:** `internal/permissions/` (368 lines)

## Summary

Port the permission manager: receives `session/request_permission` from
ACP, presents to user (via event broadcast), enforces `allow_always` /
`allow_session` policies, stale-prompt auto-deny (5min + 60s sweep),
audit log.

## Go Source

`internal/permissions/` — `Manager`, `PermissionRequest`, `PermissionResponse`,
`PermissionOption`, policy storage, stale prompt sweeper goroutine, callback
to server for event broadcast.

## Rust Implementation

- `PermissionManager` trait from S-INTERFACES
- Pending requests: `DashMap<String, PendingRequest>` with timestamps
- Stale sweeper: `tokio::time::interval` task, auto-deny after 5min
- Policies: `allow_always` / `allow_session` stored per session/tool
- Callback: `Arc<dyn Fn(PermissionRequest) + Send + Sync>` or a channel
- Port tests

## Acceptance Criteria

- [ ] Permission requests stored and retrievable
- [ ] `allow_always` / `allow_session` policies enforced
- [ ] Stale prompts auto-denied after 5 minutes
- [ ] Audit log records all decisions
- [ ] `cargo test permissions` passes
