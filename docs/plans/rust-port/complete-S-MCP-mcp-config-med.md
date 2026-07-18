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
- **Atomic write:** use shared `crate::fsutil::atomic_write` (backed by
  `atomic-write-file`: temp + fsync + mode `0600` + rename + parent fsync).
  Do not re-implement WriteFileAtomic locally.
- State dir / home: resolve via config / `crate::fsutil::home_dir` (same
  `~/.local-agent/` path as Go).
- Health: probe stdio (spawn + check), http/sse (HTTP request)
- Port `config_test.go`, `health_test.go`

## Acceptance Criteria

- [x] `mcp.json` read/modify/write behavior matches raw Go contract fixtures
- [x] Server config CRUD (add/remove/update by name)
- [x] Health status: green (healthy) / red (error) / gray (unknown)
- [x] `cargo test mcp` passes
- [x] Enabled servers reach ACP `session/new` (capability-filtered; soft-fail)

## Remaining / related

- MCP-over-ACP broker (`mcp/message`) — epic follow-on; SDK feature present but
  unused while agents rarely advertise ACP MCP transport.
- UI “restart session to apply MCP” banner — product story, not this port story.
