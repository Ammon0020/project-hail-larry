# Bump agent-client-protocol 1.2.0 → 1.3.0

> **Status:** done | **Difficulty:** easy | **Urgency:** low
> **Epic:** [profiles-over-acp](../complete-profiles-over-acp-hard.md) (loose follow-up)

## Goal

Update the core ACP SDK dependency to the latest published version to pick up
bug fixes, new unstable feature forwarding, and schema alignment.

## Changes

- `Cargo.toml`: `agent-client-protocol` 1.2.0 → 1.3.0 (also bumps
  `agent-client-protocol-derive` 1.2.0 → 1.3.0 transitively).
- Updated the dependency comment to reflect the new version and MSRV (1.88.0).

## What 1.3.0 forwards (new)

The SDK now forwards these schema features to `agent-client-protocol-schema`:
`unstable_auth_methods`, `unstable_elicitation`,
`unstable_end_turn_token_usage`, `unstable_mcp_over_acp`,
`unstable_protocol_v2`, `unstable_session_fork`.

## What 1.3.0 does NOT forward

`unstable_llm_providers` is still NOT forwarded by the SDK core crate. The
direct `agent-client-protocol-schema` dep remains necessary for Cargo feature
unification so `AgentCapabilities.providers` isn't stripped at `initialize`.
The hand-rolled `JsonRpcRequest` types in `src/acp/providers.rs` also remain.

When the SDK eventually forwards `unstable_llm_providers`, the follow-up work
is:
1. Remove the `agent-client-protocol-schema` direct dep from `Cargo.toml`.
2. Add `unstable_llm_providers` to the `agent-client-protocol` features list.
3. Replace the hand-rolled provider RPCs in `src/acp/providers.rs` with SDK
   types if they're now generated.
4. Verify `unused_crate_dependencies` stays clean.

## Verification

- `cargo build` — pass
- `cargo test -q --all-targets` — 442 passed, 0 failed
- `cargo clippy -q --all-targets -- -D warnings` — clean
- `cargo fmt --check -q` — clean

## Acceptance

- [x] Version bumped to 1.3.0
- [x] All tests, clippy, fmt pass
- [x] Comment updated to reflect new version and remaining schema dep gap
