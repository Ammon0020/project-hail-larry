# Replace Go mockagent with agent-client-protocol-test

> **Status:** pending | **Difficulty:** med | **Urgency:** med
> **Epic:** [profiles-over-acp](../complete-profiles-over-acp-hard.md) (loose follow-up)

## Goal

Replace the hand-rolled Go mock agent (`cmd/mockagent/main.go`, 327 lines — the
last Go binary in the project) with the ACP SDK's `testy` mock agent from the
`agent-client-protocol-test` crate. This eliminates the Go toolchain dependency
from the project and the out-of-band `/tmp/mockagent` build dance.

## Background

The current mock agent is a Go binary built separately and referenced by path
in `tests/acp_core_lifecycle.rs:27-30` with a manual `assert!` that it exists.
Tests can't inject per-test env vars (e.g. `MOCKAGENT_NO_MODE_CAP=1`) because
`AgentInfo` has no env field — this blocks the profile-fallback test gap
recorded in `docs/known-issues.md`.

The SDK's `agent-client-protocol-test` crate ships:
- `testy` — a mock ACP agent binary
- `mcp-echo-server` — a mock MCP server for tool testing
- Shared test utilities and fixtures

## Approach

1. **Evaluate the crate**: `agent-client-protocol-test` is **unpublished**
   (`publish = false`, v0.11.0). A git or path dependency is required:
   ```toml
   [dev-dependencies]
   agent-client-protocol-test = { git = "https://github.com/agentclientprotocol/rust-sdk", branch = "main" }
   ```
   Assess whether `testy` supports the same behaviors as the current Go
   mockagent: `[profile: <id>]` reply prefix, `MOCKAGENT_NO_MODE_CAP` env
   suppression, model switching, session lifecycle.

2. **If testy is sufficient**: Replace `cmd/mockagent/` with a test harness
   that spawns `testy` as a subprocess (or uses it as a library). Update
   `tests/acp_core_lifecycle.rs` to use the new harness. Remove the Go
   toolchain from CI and build scripts.

3. **If testy is insufficient**: Either contribute the missing behaviors
   upstream, or keep the Go mockagent but add an `env` field to `AgentInfo`
   (`src/acp/agent_registry.rs`) to unblock per-test env injection. The env
   field is needed regardless — file it as a separate story.

4. **Unblock the fallback test**: With per-test env support (via testy or the
   `AgentInfo.env` field), add the `MOCKAGENT_NO_MODE_CAP=1` fallback test
   that's deferred in `docs/known-issues.md`. Remove the known-issues entry
   and mark the S-PROF-ACP acceptance criterion `[x]` instead of `[~]`.

## Risks

- **Unpublished crate**: No version stability guarantee. A git dep means
  upstream changes can break the build. Pin to a specific commit for
  reproducibility.
- **Behavior parity**: The Go mockagent has project-specific behaviors
  (`[profile: <id>]` prefix, mode-cap suppression). `testy` may not replicate
  these without configuration or upstream changes.
- **Go removal**: If `testy` can't fully replace the Go mockagent, the Go
  toolchain stays. Don't force the migration if it adds complexity.

## Acceptance

- [ ] `cmd/mockagent/` is removed or the Go toolchain is no longer required
- [ ] All existing ACP tests pass with the new mock agent
- [ ] `MOCKAGENT_NO_MODE_CAP=1` fallback test is added (or `AgentInfo.env`
      field is filed as a separate story)
- [ ] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D warnings`,
      `cargo fmt --check -q` pass
- [ ] `make test-contract` passes

## Suggested commit

```
test(acp): replace Go mockagent with agent-client-protocol-test testy

Replace the hand-rolled Go mock agent (cmd/mockagent, the last Go binary)
with the ACP SDK's testy mock agent. Pin to a specific git commit for
reproducibility. Add the MOCKAGENT_NO_MODE_CAP fallback test that was
deferred. Remove the Go toolchain from CI.
```
