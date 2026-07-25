# HTTP listener serves full app surface (API + WS + SPA) in cleartext even when TLS is enabled

- **Difficulty:** easy
- **Urgency:** critical
- **File:** `src/app/listen.rs`
- **Lines:** 75-121 (bind/serve), 124-146 (serve_http)

## Description

`bind` *always* binds the HTTP listener (line 77-79) regardless of `tls_enabled`, and `serve` (line 106-121) runs `serve_http` with the *same* `router` as HTTPS. There is no HTTP→HTTPS redirect, no `TlsConnection`-gated refusal on the HTTP path, and no route that is HTTPS-only. The `TlsConnection` extension is inserted only on the HTTPS listener (line 155) and is consulted in exactly one place — `api/mod.rs:1227` — to set the `Secure` cookie flag on preview tokens. It is never used to reject non-TLS requests or to require TLS for sensitive operations.

Consequently, even on a correctly TLS-enabled daemon, a browser or script that hits `http://host:port/...` gets the full IDE, the full `/api/*` surface, and the `/ws` upgrade over cleartext. Bearer credentials (`api/mod.rs:1431-1438`) and WebSocket `deviceId`/`secret` query params (`sync/mod.rs:383-385`) traverse the LAN in cleartext. The module doc comment (lines 1-5) claims a failed HTTPS bind "cannot silently downgrade a TLS-enabled daemon to cleartext-only operation" — that is true only for *bind* failures; it does not address a client simply choosing `http://`.

This is the core transport-security defect: AGENTS.md mandates "Default to TLS; only bind 0.0.0.0 with TLS enabled," but the cleartext listener serves the entire authenticated surface regardless of TLS state.

## Recommendation

When `tls_enabled` is true, the HTTP listener should either (a) not be bound at all, or (b) emit only `301`/`308` redirects to the HTTPS listener, serving no API/WS/SPA content. If a loopback-only HTTP listener is needed for the host CLI (`cli/mod.rs:433` hits `http://127.0.0.1:{port}`), bind HTTP *only* on loopback when TLS is on, and serve the full router only on HTTPS. At minimum, refuse `/api/*` and `/ws` on the non-TLS listener when `tls_enabled` is true.

## Verification

`listen.rs:77-79` binds HTTP unconditionally; `listen.rs:110` serves the same `router` over HTTP; `listen.rs:114` serves it over HTTPS. `api/mod.rs:1227` is the only consumer of `TlsConnection`. Grep for `redirect`/`Location.*https`/`301`/`308` in `src/` returns no HTTP→HTTPS redirect (only an unrelated hit in `logging.rs:30`).
