# Story S-SERVER: HTTP Server & REST API

> **Phase:** 4 | **Depends on:** all other service stories | **Go source:** `internal/server/` (2,382 lines)

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

- `axum` framework (see `docs/rust-ecosystem/web-framework.md`)
- `AppState` replaces `Deps` — `Arc` of all manager traits
- Routes: 40+ routes mapping 1:1 from Go's `s.mux.HandleFunc` patterns
- Auth middleware: `tower` middleware checking Bearer header / query params
- Rate limiting: `tower::limit` or `governor` for pairing endpoints
- TLS: `axum-server` with `RustlsConfig`, self-signed cert via `rcgen`
- Dual listeners: `tokio::spawn` HTTP + HTTPS tasks
- Embedded frontend: `rust-embed` (see `docs/rust-ecosystem/build-and-embed.md`)
- SPA fallback: serve `index.html` for unmatched routes
- Port `server_test.go`, `api_test.go`, `dual_test.go`

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
- [ ] Rate limiting on pairing endpoints
- [ ] TLS with auto-generated self-signed cert
- [ ] Dual HTTP (:7337) + HTTPS (:7338) listeners
- [ ] Embedded frontend served with SPA fallback
- [ ] WebSocket endpoint wired to S-SYNC hub
- [ ] Request/response size caps enforced
- [ ] `cargo test server` passes
- [ ] Side-by-side: Rust server passes same API tests as Go server
