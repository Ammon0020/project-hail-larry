# Task: Fix workspace registration loopback bypass divergence

> **Status:** done | **Difficulty:** small | **Urgency:** med
> **Origin:** S-SERVER audit (2026-07-18). Epic: rust-port.

## Problem

The contract test `rest_workspaces_register_remote_disabled` fails against the
Rust server. The golden fixture expects HTTP 403 when
`allowRemoteWorkspaceRegistration` is false. Go
(`internal/server/api.go:353-354`) returns 403 unconditionally. Rust
(`src/api/mod.rs`) added a loopback bypass: `if !allow_remote &&
!loopback { return Err(...) }`, so the Rust server returns 201 when connected
via 127.0.0.1.

## Resolution (Option A — Go parity)

Removed the loopback exception on the remote-registration gate. When
`allowRemoteWorkspaceRegistration` is false, `POST /api/workspaces` returns
403 for all callers including loopback. Host registration stays on
`app add-folder` (config persistence), matching Go.

## Acceptance criteria

- [x] `rest_workspaces_register_remote_disabled` contract test passes
- [x] N/A — divergence removed (not intentional); no known-issues entry
