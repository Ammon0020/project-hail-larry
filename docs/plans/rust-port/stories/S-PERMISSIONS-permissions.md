# Story S-PERMISSIONS: Permission Manager

> **Phase:** 2 | **Depends on:** S-EVENTS, S-INTERFACES | **Go source:** `internal/permissions/` (368 lines)

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
- Pending requests use a request ID plus `oneshot` response sender; awaiting
  a decision never holds a manager lock.
- Stale sweeper: `tokio::time::interval` task, auto-deny after 5min.
- Policies preserve the existing session/tool/target/command matching
  semantics. Keep persistence behavior compatible in the initial port;
  durable policy storage is a post-parity enhancement.
- Publish permission requests through the narrow app-event publisher, with no
  post-construction callback setter.
- Port tests, including first-response-wins, expiry, and concurrent response
  races.

## Acceptance Criteria

- [ ] Permission requests stored and retrievable
- [ ] `allow_always` / `allow_session` policies enforced
- [ ] Stale prompts auto-denied after 5 minutes
- [ ] Ephemeral audit log records all decisions for the running daemon; durable policy/audit storage remains post-parity work
- [ ] `cargo test permissions` passes
