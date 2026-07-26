# Replace Go mockagent with a Rust port

> **Status:** complete | **Difficulty:** med | **Urgency:** med
> **Epic:** [profiles-over-acp](../complete-profiles-over-acp-hard.md) (loose follow-up)

## Goal

Replace the hand-rolled Go mock agent (`cmd/mockagent/main.go`, 339 lines — the
last Go binary in the project) with a Rust binary that preserves all 5
project-specific behaviors. This eliminates the Go toolchain dependency from
the project and the out-of-band `/tmp/mockagent` build dance.

## Background

The mock agent was a Go binary built separately and referenced by path in
`tests/acp_core_lifecycle.rs` with a manual `assert!` that it exists. Per-test
env vars (`MOCKAGENT_NO_MODE_CAP=1`, `MOCKAGENT_EXIT_AFTER_INIT=1`) were injected
via an `env`-wrapper-as-command hack because `AgentInfo` has no env field.

The original plan proposed adopting the ACP SDK's `testy` mock agent from the
`agent-client-protocol-test` crate. Investigation found testy supports **0 of 5**
project-specific behaviors natively (`[profile: <id>]` prefix, mode-cap
suppression, post-init exit, real shell tool calls, per-session profile
recording), and the crate is unpublished (`publish = false`) — a git dep would
add supply-chain risk without solving the parity gaps.

## Approach (as executed)

A faithful Rust port was chosen over testy adoption: same behaviors, no new
external dependency (uses the already-vendored `agent-client-protocol` v1.3.0),
no upstream PR dependency.

1. **Agent-side SDK spike (GATE)**: confirmed `agent-client-protocol` v1.3.0
   exposes the full agent-side API (`Agent.builder()`, `on_receive_request`,
   `Stdio` transport, `SessionConfigOption` with `Category: Mode`,
   `ToolCall`/`ToolCallUpdate`, `SessionUpdate::AgentMessageChunk`). Built and
   cleaned up a spike binary; no blockers.
2. **Port**: translated `cmd/mockagent/main.go` to `src/bin/mockagent.rs`
   (442 lines), preserving all 5 behaviors byte-for-byte. Added a `[[bin]]`
   entry to `Cargo.toml` (not feature-gated).
3. **CI/Makefile**: removed `actions/setup-go` and `go build` steps from
   `.github/workflows/rust-ci.yml` (Linux `test` + Windows `build-test-windows`
   jobs); replaced with `cargo build --bin mockagent --locked` + copy to the
   path tests expect. Updated the `Makefile` `mockagent` target.
4. **Go removal**: deleted `cmd/mockagent/`, `cmd/`, `go.mod`, `go.sum`, and a
   stray `mockagent.exe`. Anchored `.gitignore`'s `bin/` rule to `/bin/` so it
   no longer swallows `src/bin/`.
5. **Docs**: updated `AGENTS.md`, `README.md`, `docs/STATUS.md`,
   `docs/rust-ecosystem/acp-rust-sdk.md`, `docs/specs/backend-spec.md`, and
   test error messages to reference the Rust binary.

## Risks (realized and mitigated)

- **`testy` parity gap**: mitigated by choosing the Rust port over testy.
- **`.gitignore` swallowed `src/bin/`**: the unanchored `bin/` rule ignored
  `src/bin/mockagent.rs`; fixed by anchoring to `/bin/`.
- **`SessionConfigOption` category**: the port explicitly sets
  `.category(SessionConfigOptionCategory::Mode)` so the client's
  `find_profile_config_id` gate matches.

## Acceptance

- [x] `cmd/mockagent/` is removed and the Go toolchain is no longer required
- [x] All existing ACP tests pass with the Rust mock agent (468 lib + 6
      lifecycle + 8 spike)
- [x] `MOCKAGENT_NO_MODE_CAP=1` fallback test passes
      (`prompt_injection_fallback_skips_set_config_option`); the
      `MOCKAGENT_EXIT_AFTER_INIT` test
      (`unexpected_actor_exit_transitions_session_to_failed`) also passes
- [x] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D warnings`,
      `cargo fmt --check -q` pass
- [ ] `make test-contract` — not run as part of this task (contract suite does
      not spawn the mockagent; unaffected by the port)

## Suggested commit

```
test(acp): replace Go mockagent with a Rust port

Port cmd/mockagent/main.go (the last Go binary) to src/bin/mockagent.rs,
preserving all 5 project-specific behaviors: the [profile: <id>] reply
prefix, MOCKAGENT_NO_MODE_CAP suppression, MOCKAGENT_EXIT_AFTER_INIT
post-init crash, word-by-word streaming with real ls/pwd tool calls, and
per-session profile recording. Remove the Go toolchain from CI and the
Makefile. Anchor .gitignore's bin/ rule to /bin/ so src/bin/ is tracked.
```
