# Story S-SYNC: WebSocket Sync Hub

> **Phase:** 4 | **Depends on:** S-EVENTS, S-PAIRING | **Go source:** `internal/sync/` (347 lines)

## Summary

Port the WebSocket hub: client registry, event broadcast, keepalive
ping/pong, reconnection sync (exp. backoff + jitter), auth-gated handshake.

## Go Source

`internal/sync/sync.go` — `Hub` (client map, auth checker, lifecycle
context), `Client` (per-connection pumps), keepalive (30s ping, 10s
timeout), broadcast to all clients, reconnection sync.

## Rust Implementation

- `axum::extract::ws` for WebSocket (see
  `docs/rust-ecosystem/web-framework.md`)
- Live fan-out: `tokio::sync::broadcast` (bounded); lagged receivers must
  resynchronize from the event store rather than silently losing events.
- Client registry / per-connection senders: stable **`dashmap` 6.x** (not an
  RC) — do not hand-roll a global `Mutex<HashMap>` hub map.
- Keepalive: `tokio::time::interval(30s)` ping task per client
- Auth: `AuthChecker` callback (delegates to S-PAIRING) — gate handshake
- Reconnection: subscribe first, replay events after the supplied cursor,
  deduplicate by durable event ID, then continue with live broadcast.
- Cancellation: `CancellationToken` for hub shutdown

## Acceptance Criteria

- [x] Paired devices can connect via WebSocket
- [x] Events broadcast to all connected clients
- [x] Keepalive ping/pong works (dead clients removed)
- [x] Reconnection sync: missing events replayed on reconnect
- [x] Unpaired devices rejected at handshake
- [x] Hub shutdown drains all connections
- [x] Slow/lagged clients resynchronize without silent event loss
- [x] Replay-to-live handoff cannot omit or duplicate durable event IDs
- [x] `cargo test sync` passes

## Deferred (out of scope for hub-only story)

- S-SERVER mount of `/ws` into the daemon HTTP stack
- Frontend reconnect exp. backoff + jitter (already in `useBackend`)
- In-flight permission re-presentation on reconnect
- Black-box contract runner against a Rust binary (needs S-SERVER)
