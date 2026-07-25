# peer_addr_string defaults to loopback when ConnectInfo is absent — silent auth bypass if misconfigured

- **Difficulty:** hard
- **Urgency:** low
- **File:** `src/api/mod.rs`
- **Lines:** 1479-1486, 1212, 1381

## Description

`peer_addr_string` returns `"127.0.0.1:0"` whenever `ConnectInfo<SocketAddr>` is missing from the request extensions (line 1485). `authorize_request` treats any loopback peer as fully trusted — no device credential required (line 1381-1391). Today this is safe because `listen.rs` correctly uses `into_make_service_with_connect_info` on both listeners (lines 129, 156). However, this is a latent footgun: any future refactor that swaps in `into_make_service` (without connect info), or any deployment behind a reverse proxy that strips the extension, would cause **every** request — including remote LAN requests — to be classified as loopback and bypass device authentication entirely, leaving only the Origin check (which a same-host browser or non-browser client can satisfy). The default is fail-open, not fail-closed.

## Recommendation

Fail closed in production: when `ConnectInfo` is absent, return a 500/403 instead of defaulting to loopback. Keep the loopback default only behind a `#[cfg(test)]` gate or an explicit test-only override.

## Verification

`peer_addr_string` line 1485 `unwrap_or_else(|| "127.0.0.1:0".to_string())`; `authorize_request` line 1381 `if is_loopback_addr(remote_addr) { ... return Ok(()) }` — no credential check on that path.
