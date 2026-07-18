# Task: Fix workspace registration loopback bypass divergence

> **Status:** pending | **Difficulty:** small | **Urgency:** med
> **Origin:** S-SERVER audit (2026-07-18). Epic: rust-port.

## Problem

The contract test `rest_workspaces_register_remote_disabled` fails against the
Rust server. The golden fixture expects HTTP 403 when
`allowRemoteWorkspaceRegistration` is false. Go
(`internal/server/api.go:353-354`) returns 403 unconditionally. Rust
(`src/api/mod.rs:407-412`) added a loopback bypass: `if !allow_remote &&
!loopback { return Err(...) }`, so the Rust server returns 201 when connected
via 127.0.0.1.

This is a genuine behavioral divergence from Go, not recorded in
`docs/known-issues.md`.

## Scope

- Decide: should loopback bypass workspace registration remote-disable?
  - **Option A (Go parity):** remove the loopback bypass — 403 regardless of
    caller. Simplest, matches Go, fixes the contract test.
  - **Option B (intentional divergence):** keep the bypass, update the golden
    fixture, record in known-issues.
- Implement the chosen option
- Verify the contract test passes

## Acceptance criteria

- [ ] `rest_workspaces_register_remote_disabled` contract test passes
- [ ] If divergence is intentional, recorded in `docs/known-issues.md`

## Out of scope

- Other contract test failures (none known at audit time)
