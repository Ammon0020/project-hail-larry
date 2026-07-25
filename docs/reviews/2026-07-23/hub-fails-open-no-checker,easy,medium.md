# Hub fails open when auth checker is unset — loopback + forgeable Origin = any local process connects

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/sync/mod.rs`
- **Lines:** 104, 296-310, 351-397

## Description

The hub is constructed with `auth: RwLock::new(None)` (line 104). `authorize_handshake` (line 296) only enforces credentials `if let Some(checker) = auth` — when `None`, auth is **completely skipped** and only the Origin check remains. The Origin header is trivially forgeable by any non-browser client (curl, scripts), and on loopback the auth bypass already applies (line 297), so a local process need only send `Origin: http://localhost:<port>` and `Host: localhost:<port>` to connect with zero credentials and begin receiving the full event stream (see the unscoped-replay finding). The doc comment (line 61-62) acknowledges this is intentional for tests, but the default is fail-open: if `set_auth_checker` is ever not called, or called after the router is served, or the checker closure panics leaving the slot stale, the hub silently operates in open mode. Production wiring (`api/mod.rs:245`) does call `set_auth_checker` before `into_router` (line 265), so there is no current window — but the design is fragile.

## Recommendation

Make the default fail-closed: change `authorize_handshake` to reject all non-loopback connections when `auth` is `None` (return 403/503 "auth not configured"). Keep loopback bypass explicit and documented. Alternatively, make `auth` non-optional and require it at construction.

## Verification

`with_bus` (line 104) sets `auth: RwLock::new(None)`. `authorize_handshake` (line 296) — `if let Some(checker) = auth { ... }` — the `None` arm falls through to only the Origin check (line 306). `handle_ws` (line 363-369) reads `auth` and passes `auth.as_ref()` (which can be `None`) to `authorize_handshake`.
