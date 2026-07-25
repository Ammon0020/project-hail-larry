# WebSocket upgrade permitted over plain ws:// on a TLS-enabled daemon

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/sync/mod.rs`
- **Lines:** 351-397

## Description

`handle_ws` accepts a `WebSocketUpgrade` and performs `authorize_handshake` based on `ConnectInfo(addr)` and `deviceId`/`secret` query params. It never inspects `TlsConnection`. Because the `/ws` route is registered on the shared router (`sync/mod.rs:197-198`) which is served on both the HTTP and HTTPS listeners (see the http-listener-serves-full-surface finding), a remote LAN client can open `ws://host:port/ws?deviceId=...&secret=...` and the credentials are sent in cleartext over the HTTP listener. There is no `wss`-only enforcement. This compounds the HTTP-listener finding specifically for the sync channel.

## Recommendation

Insert `Extension<TlsConnection>` into the `/ws` handler and reject the upgrade (e.g. 403) when `tls_enabled` is true and the request did not arrive over the TLS listener. Alternatively, only mount the `/ws` route on the HTTPS router.

## Verification

`sync/mod.rs:354` extracts `ws: WebSocketUpgrade` and `ConnectInfo(addr)` but no `TlsConnection`. `sync/mod.rs:380-390` calls `authorize_handshake` with no transport check. `sync/mod.rs:197-198` registers `/ws` on the hub router merged into the shared router at `api/mod.rs:265`.
