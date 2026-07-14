# Story S-MCP: MCP Config & Health

> **Phase:** 3 | **Depends on:** — | **Go source:** `internal/mcp/` (535 lines)

## Summary

Port MCP (Model Context Protocol) config management: Claude-Desktop-
compatible `mcp.json` read/write (raw JSON, preserving formatting/comments),
health status endpoint, server config CRUD.

## Go Source

`internal/mcp/config.go`, `internal/mcp/health.go` — config file at
`~/.local-agent/mcp.json`, raw JSON round-trip (no typed deserialization
to preserve formatting), health checks for stdio/http/sse transports.

## Rust Implementation

- Config: `serde_json::Value` for raw JSON round-trip (do NOT deserialize
  into typed structs — formatting/comments would be lost)
- Atomic write: temp + rename
- Health: probe stdio (spawn + check), http/sse (HTTP request)
- Port `config_test.go`, `health_test.go`

## Acceptance Criteria

- [ ] `mcp.json` read/written with formatting preserved
- [ ] Server config CRUD (add/remove/update by name)
- [ ] Health status: green (healthy) / red (error) / gray (unknown)
- [ ] `cargo test mcp` passes
