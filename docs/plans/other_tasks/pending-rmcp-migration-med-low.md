# Adopt `agent-client-protocol-rmcp` + bump ACP SDK

> Chore. Difficulty: medium. Urgency: low (current code works).
> Branch: `chore/rmcp-migration`.

## Why

`src/mcp/tools.rs` hand-rolls Content-Length-delimited JSON-RPC for MCP
`tools/list` over stdio and HTTP — ~600 lines of duplicated protocol plumbing
that the maintained `agent-client-protocol-rmcp` crate already provides via
`rmcp` integration. Aligns with AGENTS.md ("ACP is the only agent integration
boundary; avoid duplicated code"). Also bumps the core SDK to pick up fixes
that may retire the hand-rolled provider RPC types in `src/acp/providers.rs`.

## Scope

In scope:
- Bump `agent-client-protocol` 1.2.0 → 1.3.0 (released 2026-07-20) in
  `Cargo.toml`. Re-check whether 1.3.0 forwards the schema
  `unstable_llm_providers` feature; if so, delete the hand-rolled wire types in
  `src/acp/providers.rs:142-171` and route through the SDK's typed enum.
- Add `agent-client-protocol-rmcp` dependency.
- Replace the JSON-RPC wire layer in `src/mcp/tools.rs` (`LiveToolLister`,
  `list_tools_stdio`, `list_tools_http`) with the rmcp-based client.

Out of scope (do not pull in):
- `agent-client-protocol-tokio` — current `async-process` + process-group
  isolation in `src/acp/core.rs:1194-1232` and `src/procutil/mod.rs` is
  security-critical; keep unless upstream confirms equivalent isolation.
- `agent-client-protocol-conductor` — no proxy chaining exists; speculative.
- `agent-client-protocol-test` — Go `cmd/mockagent` is functional and gives
  cross-language-SDK interop coverage for free.

## Files to touch

- `Cargo.toml` — bump + add dep.
- `src/mcp/tools.rs` — primary rewrite target (734 lines).
- `src/mcp/mod.rs:166-219` — MCP server → ACP type conversion; verify still
  needed once rmcp is in.
- `src/acp/providers.rs:6-171` — delete hand-rolled types if 1.3.0 forwards the
  feature.
- `tests/` — any MCP tool-listing tests.

## Preserve (app concerns layered on top of the wire layer)

- Tool-catalog caching and invalidation in `src/mcp/tools.rs`.
- Profile-based tool filtering.
- HTTP + SSE transport support — verify rmcp covers both before committing; if
  rmcp is stdio-only, keep HTTP path hand-rolled and document why in
  `docs/known-issues.md`.
- Error shape / messages surfaced to the REST API in `src/api/mcp.rs`.

## Verification

- `cargo test -q --all-targets`
- `cargo clippy -q --all-targets -- -D warnings`
- `cargo fmt --check -q`
- `make test-contract` (touches `/api/mcp` surface)
- Manual: stdio MCP server tool list, HTTP MCP server tool list, cached
  re-list, profile-filtered list.

## Acceptance

- `src/mcp/tools.rs` no longer contains hand-rolled Content-Length framing or
  `tools/list` JSON-RPC.
- All verification steps green.
- No regression in tool-listing behavior across stdio/HTTP/cache/profile paths.
- If HTTP transport can't move to rmcp, the gap is recorded in
  `docs/known-issues.md` and the stdio path still migrates.

## Hand-off

Suggested commit: `chore(mcp): adopt agent-client-protocol-rmcp, bump ACP SDK`
