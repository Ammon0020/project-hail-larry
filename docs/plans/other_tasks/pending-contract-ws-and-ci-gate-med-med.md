# Task: Contract suite — WS replay tests and CI parity gate

> **Status:** pending | **Difficulty:** med | **Urgency:** med
> **Origin:** S-CONTRACT audit (2026-07-18). Epic: rust-port.

## Problem

`docs/plans/rust-port/active-S-CONTRACT-compatibility-med.md` has two unchecked
ACs with real gaps confirmed by audit:

1. **AC 3 — WebSocket tests:** Golden fixtures capture auth-rejection (401),
   origin-rejection (403), and live broadcast, but the Rust runner only
   exercises origin-rejection + connection success. **Replay/live transition
   (`?after=`) has no fixture and no runner code.** Slow-client recovery is
   not tested. Auth-rejection is skipped (needs non-loopback).

2. **AC 6 — Parity gate:** The contract suite is advisory only — documented in
   AGENTS.md/Makefile/STATUS.md but **not enforced in CI**. The `contract`
   feature gate keeps it out of `cargo test --all-targets`. A Rust change can
   break wire compatibility without CI catching it.

## Scope

### WS replay/live transition tests
- Add a golden fixture for `?after=` replay (connect with cursor, verify
  replayed events, verify transition to live delivery)
- Add runner code in `tests/contract_runner/ws.rs` to exercise replay
- Add slow-client recovery test (or document why it's not feasible in a
  black-box harness)
- Wire auth-rejection runner test (needs non-loopback — may require harness
  extension to bind a non-loopback address)

### CI parity gate
- Add the contract suite to `.github/workflows/rust-ci.yml`:
  `CONTRACT_BACKEND=rust cargo test --features contract -p contract_runner`
- Decide: run on every push, or only on merge to main / release tags
- Ensure the Go binary is built for golden regeneration in CI (or cache
  checked-in goldens and only regenerate manually)

## Acceptance criteria

- [ ] `?after=` replay fixture + runner test exists and passes
- [ ] Live-broadcast runner test exists and passes (currently skipped)
- [ ] Auth-rejection runner test exists and passes (or documented why
      non-loopback isn't feasible in CI)
- [ ] Contract suite runs in CI on at least Linux
- [ ] A Rust change that breaks wire compatibility fails CI

## Out of scope

- Slow-client recovery if it proves infeasible in a black-box harness
  (document and move on)
- CLI golden differential testing (deliberately excluded by design —
  `main.rs:58-61`)
