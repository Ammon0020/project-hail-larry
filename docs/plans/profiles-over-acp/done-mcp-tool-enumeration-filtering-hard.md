# Story S-PROF-TOOLS: MCP Tool Enumeration + Per-Profile Filtering

> **Status:** done | **Difficulty:** hard
> **Epic:** [profiles-over-acp](../complete-profiles-over-acp-hard.md).
> **Depends on:** S-PROF-CONFIG | **Blocks:** S-PROF-UI (tool checkboxes).

> **Superseded 2026-07-22:** Stable ACP attaches whole MCP servers at session
> creation. The tool catalog and per-tool policy were removed in favor of the
> profile-level `mcpServers` allowlist; see
> `docs/plans/other_tasks/active-profile-mcp-transition-hard-high.md`.

## Goal

Enumerate the individual tools each enabled MCP server exposes (via `tools/list`,
cached), then restrict a session's MCP tool set to the intersection of the
capability-filtered tools and the active profile's `tools` whitelist.

## Background / current behavior

- Tools are MCP servers configured globally in `src/mcp/mod.rs` and filtered per
  session by agent capabilities in `load_session_mcp_servers`
  (`src/acp/core.rs:1410-1413, 1575, 2299-2319`). There is no per-tool
  (sub-server) filtering today.
- The profile schema (S-PROF-CONFIG) carries a per-tool whitelist of individual
  tool names; enforcing it requires knowing which tools exist.

## Desired behavior

- New enumeration helper (in `src/mcp/mod.rs` or a new `src/mcp/tools.rs`) that
  calls `tools/list` on each enabled MCP server and returns
  `{ server → [tool names] }`. Result is **cached** (invalidated on MCP config
  change); enumeration failure for one server is isolated and logged, not fatal.
- `load_session_mcp_servers` intersects the capability-filtered tool set with
  the active profile's whitelist. An empty whitelist means "profile allows no
  extra tools" (explicit) — do NOT treat empty as "allow all"; use a documented
  sentinel (e.g. absence of the `tools` key vs `[]`) decided in the schema.
- Whitelist entries that reference tools not currently present are ignored
  (logged), not errors — servers can be offline.

## Acceptance criteria

- [ ] Enumeration returns per-server tool names for the mock/inline MCP servers
      and is cached (second call within TTL/until invalidation does not re-hit
      `tools/list`).
- [ ] One server failing `tools/list` does not break enumeration for others
      (logged, isolated).
- [ ] With a profile whitelisting a subset of tools, `load_session_mcp_servers`
      exposes only whitelisted ∩ capability-allowed tools to the session.
- [ ] Empty-vs-absent `tools` semantics are enforced and documented in code.
- [ ] Stale whitelist entries (unknown tool) are ignored with a log line.
- [ ] Cache invalidation on MCP config change is covered by a test.
- [ ] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D
      warnings`, `cargo fmt --check -q` clean.

## Out of scope

- UI to pick tools (S-PROF-UI) — this story exposes enumeration to the API layer.
- Removing/adding MCP servers; per-server (not per-tool) filtering already exists.

## Notes

- Enumeration timing/caching strategy (session-setup vs config-time vs lazy) and
  TTL are an OpenItems decision — pick a default here and record it.
