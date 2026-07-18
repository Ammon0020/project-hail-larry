# Story S-SERVER: HTTP Server & REST API

> **Phase:** 4 | **Status:** complete (2026-07-18) | **Depends on:** S-CONTRACT,
> S-MIGRATE, S-SYNC, all service stories | **Go source:** `internal/server/`

## Summary

Port the HTTP server: all REST API routes, auth middleware, rate limiting,
embedded frontend serving, dual HTTP/HTTPS listeners, TLS with self-signed
cert auto-generation, WebSocket endpoint wiring.

## Rust Implementation

- `src/api/` — Axum router (`router()`), handlers, auth (`authorize_request`),
  pairing rate limit (5/min burst 5), SPA fallback → `embed.rs` (`rust-embed`).
- `src/app/listen.rs` — dual HTTP/HTTPS bind+serve, timeouts/concurrency;
  `tls_cert.rs` — `rcgen` self-signed certs; `tls.rs` — rustls provider.
- `src/sync` hub merged as `GET /ws` (query auth + Origin gate).
- Body caps: 10 MiB API / 50 MiB file-write; loopback Origin on mutations;
  no loopback bypass for remote workspace registration when disabled.

## Acceptance Criteria

- [x] All 40+ routes respond with identical JSON shapes to Go server
  — `src/api/mod.rs` `router()`; verified by S-CONTRACT goldens (CI `contract`
  job; suite green after loopback register fix).
- [x] Auth middleware rejects unpaired devices (except pairing routes)
  — `authorize_request` + unit tests; contract auth/WS rejection cases.
- [x] Bounded governor rate limiting matches the Go pairing policy values
  — token bucket `PAIR_RATE_PER_MINUTE=5` / `PAIR_RATE_BURST=5` (Go parity);
  `pair_request_bucket_allows_a_five_request_burst` + 429 handler test.
- [x] TLS with auto-generated self-signed cert — `tls_cert::ensure_self_signed`.
- [x] Dual HTTP (:7337) + HTTPS (:7338) listeners — `app::listen::bind`/`serve`.
- [x] Embedded frontend served with SPA fallback — `api::embed` + `spa_fallback`.
- [x] WebSocket endpoint wired to S-SYNC hub — `hub.into_router()` merge.
- [x] Request/response size caps and timeout/concurrency limits match contract
  fixtures — `MAX_API_BODY_BYTES` / `FILE_WRITE_MAX_BODY_BYTES`; listen timeouts.
- [x] Loopback CSRF/Origin and WebSocket Origin behavior match Go fixtures
  — `loopback_origin_allowed`; contract `ws_origin_rejection` + API Origin gate.
- [x] `cargo test` for server modules passes — `cargo test --lib api::` (25) and
  `app::` (11) green (2026-07-18).
- [x] Side-by-side: Rust server passes same API tests as Go server
  — S-CONTRACT closed; CI contract job enforces Rust black-box suite.

## Verification

```
cargo test -q --lib api::
cargo test -q --lib app::
cargo test -q --test contract_runner --features contract -- --test-threads=1
```
