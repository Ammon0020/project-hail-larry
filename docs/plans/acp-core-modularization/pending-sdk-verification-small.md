# Story S-ACP-MOD-VERIFY: Validate SDK Boundaries and Diagnostic Tooling

> **Status:** pending | **Difficulty:** small
> **Epic:** [ACP Core Modularization](../pending-acp-core-modularization-hard.md).
> **Depends on:** S-ACP-MOD-REGISTRY.

## Goal

Verify the split against the current ACP SDK and decide, with a small proof or
documented rejection, whether development-only SDK test or trace tooling adds
coverage value.

## Acceptance criteria

- [ ] Confirm the extracted SDK builder/handler boundary against current
      `agent-client-protocol` documentation using Context7.
- [ ] Evaluate `agent-client-protocol-test` for an actor/handler fixture; add
      it only when it replaces meaningful bespoke test plumbing.
- [ ] Evaluate trace viewer with a representative interoperability trace; keep
      it development-only or document why it is not adopted.
- [ ] Reconfirm that HTTP transport, rmcp, conductor, and polyfill remain out
      of scope unless a concrete product topology requires them.
- [ ] Record the decision in the epic and update `docs/STATUS.md` if scope or
      a deferred gap changes.
- [ ] Run relevant Rust checks; do not add runtime dependencies solely for
      diagnostics.

## Out of scope

Implementing MCP-over-ACP brokerage, an HTTP ACP listener, or proxy chains.
