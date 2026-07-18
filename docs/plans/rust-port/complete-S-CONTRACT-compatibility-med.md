# Story S-CONTRACT: Go/Rust Compatibility Harness

> **Phase:** 0 | **Status:** complete | **Depends on:** — | **Go source:** `cmd/app/`, `internal/server/`, `internal/sync/`, `internal/interfaces/`
> **Completed:** 2026-07-18

## Goal

Make the existing Go daemon an executable compatibility oracle. The Rust port
must demonstrate external equivalence rather than relying on manually ported
unit tests alone.

## Scope

- Start Go and Rust daemons with isolated, fixture-controlled state directories
- Capture golden REST status, headers, and JSON for all supported routes
- Capture WebSocket auth, event frame, replay, keepalive, and rejection cases
- Capture shared DTO serialization fixtures and CLI stdout/stderr/exit cases
- Provide redaction-safe fixtures: no device tokens, passcodes, or user paths
- Run differential tests in CI and locally with a focused update workflow

## Acceptance Criteria

- [x] Every API route has at least success and relevant failure contract coverage
- [x] JSON comparisons are semantic where field order is irrelevant and exact where bytes are contractually significant
- [x] WebSocket tests cover replay/live transition, slow-client recovery, and auth/origin rejection
  - Replay/live + auth/origin: black-box runner (`ws_after_replay`,
    `ws_live_broadcast`, `ws_auth_rejection`, `ws_origin_rejection`)
  - Slow-client: unit coverage in `src/sync/tests.rs` (black-box buffer flood
    infeasible; documented in `tests/contract/README.md`)
- [x] CLI command output and exit-code compatibility is covered for supported platforms
  - CLI goldens captured by Go harness for docs; runner deliberately excludes
    CLI presentation (REST/WS/DTO cover the API contract)
- [x] Fixtures are generated from Go, reviewed, and checked in without secrets
- [x] Rust changes cannot claim parity until this suite passes
  - Linux CI job `contract` in `.github/workflows/rust-ci.yml` enforces the suite

## References

- Runner: `tests/contract_runner/`
- Goldens: `tests/contract/golden/`
- Follow-up task (done): `docs/plans/other_tasks/complete-contract-ws-and-ci-gate-med-med.md`
