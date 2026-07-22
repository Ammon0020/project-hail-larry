# Story S-PROF-MOCK: Mock Agent Honors set_config_option Mode

> **Status:** pending | **Difficulty:** easy
> **Epic:** [profiles-over-acp](../pending-profiles-over-acp-hard.md).
> **Depends on:** — | **Blocks:** S-PROF-ACP (contract coverage).

## Goal

Make `cmd/mockagent` advertise and honor `session/set_config_option` for the
`mode` category so the ACP send path (S-PROF-ACP) can be verified by contract
tests, including the capability-gated fallback branch.

## Background / current behavior

- `cmd/mockagent/main.go:162-164` implements `SetSessionMode` as a no-op and
  ignores modes/profiles.
- Real client send path will use `SetSessionConfigOptionRequest { category:
  Mode, option: "profile", value: <id> }` (types imported in
  `src/acp/providers.rs:22-25`).

## Desired behavior

- Mock agent advertises the `session/set_config_option` capability with a
  `mode` category `SessionConfigOption` (id `profile`) during initialize/session
  setup so the daemon's capability gate takes the ACP branch.
- Mock records the last received config option (category + option + value) and
  exposes it for assertion (e.g. echoed in a subsequent update or a test hook /
  log line the contract harness can read).
- A configuration/env toggle to run the mock WITHOUT advertising the capability,
  so the prompt-injection fallback branch is also exercisable.

## Acceptance criteria

- [ ] Mock advertises the `mode`-category config option and accepts
      `session/set_config_option` without error.
- [ ] Last-set profile value is observable to a test (assertable via harness).
- [ ] A no-capability mode exists so fallback (prompt injection) can be tested.
- [ ] Existing mockagent tests still pass; `make test-contract` green.

## Out of scope

- Deprecated `session/set_mode` / `current_mode_update` support.
- Any real behavior change based on the profile inside the mock.
