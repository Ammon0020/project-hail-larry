# Story S-SERVER: HTTP Server & REST API

> **Phase:** 4 | **Depends on:** S-CONTRACT, S-MIGRATE, S-SYNC, all service stories | **Go source:** `internal/server/` (2,382 lines)

## Summary

Port the HTTP server: all REST API routes, auth middleware, rate limiting,
embedded frontend serving, dual HTTP/HTTPS listeners, TLS with self-signed
cert auto-generation, WebSocket endpoint wiring.

## Go Source

`internal/server/` — `Server`, `Deps`, `routes()` (40+ routes), auth
middleware (`requireAuth`), rate limiting (`withPairRateLimit`), TLS
(`tls.go`), embedded frontend (`go:embed dist`), API handlers
(`api.go`), MCP endpoints (`mcp.go`), provider endpoints (`providers.go`).

## Rust Implementation

- `axum` router and tower middleware (see `docs/rust-ecosystem/web-framework.md`).
- `AppState` contains explicitly constructed narrow services; do not require
  trait-object indirection where a concrete service is sufficient.
- Routes: 40+ routes map 1:1 from Go patterns and are verified by S-CONTRACT.
- Auth middleware preserves Bearer/query auth, loopback bypass, mutating
  request Origin checks, and WebSocket Origin checks.
- Rate limiting uses a bounded governor-based policy for pairing endpoints.
- TLS deliberately selects one rustls crypto provider before listener startup;
  the Phase 0 decision selects `axum-server` or `tokio-rustls` serving.
- Self-signed cert generation: **`rcgen`** (write via `crate::fsutil::atomic_write`
  into `tlsCertDir`).
- Dual listeners: coordinated `tokio::spawn` HTTP + HTTPS tasks with one
  cancellation root.
- Embedded frontend: **`rust-embed`**; SPA fallback serves `index.html` only
  after API and static-asset route handling.
- Apply request and response size caps plus timeouts/concurrency limits as
  documented layers, then port server/API/dual tests.

## Route Inventory (from server.go)

- `GET /health` (no auth)
- `POST /api/pair/*` (rate-limited, no auth)
- `GET/DELETE /api/devices` (auth)
- `GET/POST /api/workspaces`, `GET/POST /api/workspaces/{id}/*` (auth)
- `GET /api/events`, `GET /api/events/{sessionId}` (auth)
- `GET/POST/DELETE /api/agents`, `POST /api/agents/autodetect` (auth)
- `GET/POST/PATCH/DELETE /api/sessions/{id}/*` (auth)
- `GET/PUT/DELETE /api/sessions/{id}/providers/*` (auth)
- `GET/POST /api/permissions/*` (auth)
- `GET/PUT/PATCH /api/mcp/*` (auth)
- `GET /ws` (auth via query params)

## Acceptance Criteria

- [ ] All 40+ routes respond with identical JSON shapes to Go server
- [ ] Auth middleware rejects unpaired devices (except pairing routes)
- [ ] Bounded governor rate limiting matches the Go pairing policy values
- [ ] TLS with auto-generated self-signed cert
- [ ] Dual HTTP (:7337) + HTTPS (:7338) listeners
- [ ] Embedded frontend served with SPA fallback
- [ ] WebSocket endpoint wired to S-SYNC hub
- [ ] Request/response size caps and timeout/concurrency limits match contract fixtures
- [ ] Loopback CSRF/Origin and WebSocket Origin behavior match Go fixtures
- [ ] `cargo test server` passes
- [ ] Side-by-side: Rust server passes same API tests as Go server
