# Remove obsolete MCP tool catalog

> Chore. Difficulty: small. Urgency: medium. Completed: 2026-07-22.

## Result

Removed the unused `tools/list` catalog, its API-state plumbing, and cache
invalidation. Profiles now select complete MCP servers; no product surface
consumed per-tool discovery or could enforce the result.

The related rmcp migration is recorded as superseded because the adapter is not
a replacement MCP client and there is no remaining catalog to port.

## Verification

- `cargo fmt --check -q`
- `cargo test -q --all-targets` — 456 passed
- `cargo clippy -q --all-targets -- -D warnings`
- `make test-contract` — 74 passed, 2 ignored

Suggested commit: `refactor(mcp): remove obsolete tool catalog`
