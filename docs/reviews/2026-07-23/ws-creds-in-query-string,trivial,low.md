# WebSocket credentials passed as query parameters leak into browser history / proxies / Referer

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `src/sync/mod.rs`
- **Lines:** 269-278, 380-387

## Description

`WsQuery` carries `device_id` and `secret` as URL query parameters on `GET /ws`. This is inherent to the browser WebSocket API (which cannot set `Authorization` headers on the upgrade request), and the code comment (`api/mod.rs:241-242`) acknowledges it. The daemon's own `debug!` log (line 388) correctly logs only `%status` and `%remote`, not the URL — so the daemon does not log the secret. However, the credential still appears in browser history, any reverse proxy access log in front of the daemon, and potentially in `Referer` headers if the WS page makes same-origin navigations. This is an accepted risk but worth documenting.

## Recommendation

Use a short-lived single-use WS ticket: the client POSTs to an authenticated HTTP endpoint to obtain a one-time token, then opens `ws://.../ws?ticket=<single-use>`, which the hub validates and invalidates. This bounds the leak window to seconds and prevents replay from history.

## Verification

`WsQuery` (line 271-278) — `device_id: Option<String>`, `secret: Option<String>` — deserialized from `Query(query)` (line 357). `authorize_handshake` (line 298-302) reads them as raw `&str`.
