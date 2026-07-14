# Story S-MCP: MCP Config & Health

> **Phase:** 3 | **Depends on:** S-CONFIG | **Go source:** `internal/mcp/` (535 lines)

## Summary

Port MCP (Model Context Protocol) config management: Claude-Desktop-
compatible `mcp.json` read/write (raw JSON, preserving formatting/comments),
health status endpoint, server config CRUD.

## Go Source

`internal/mcp/config.go`, `internal/mcp/health.go` — config file at
`~/.local-agent/mcp.json`, raw JSON round-trip (no typed deserialization
to preserve formatting), health checks for stdio/http/sse transports.

## Rust Implementation

- Preserve the raw config bytes for read/modify/write behavior. Do not use
  `serde_json::Value` as a formatting-preserving round trip, and do not claim
  comment preservation for strict JSON; contract fixtures define the existing
  behavior.
- Atomic write: temp + rename
- Health: probe stdio (spawn + check), http/sse (HTTP request)
- Port `config_test.go`, `health_test.go`

## Acceptance Criteria

- [ ] `mcp.json` read/modify/write behavior matches raw Go contract fixtures
- [ ] Server config CRUD (add/remove/update by name)
- [ ] Health status: green (healthy) / red (error) / gray (unknown)
- [ ] `cargo test mcp` passes
