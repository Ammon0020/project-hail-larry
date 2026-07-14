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
- Use a bounded broadcast channel for durable events and track only
  connection-specific state needed for shutdown.
- A lagged receiver must explicitly resynchronize from the event store rather
  than silently losing events.
- Keepalive: `tokio::time::interval(30s)` ping task per client
- Auth: `AuthChecker` callback (delegates to S-PAIRING) — gate handshake
- Reconnection: subscribe first, replay events after the supplied cursor,
  deduplicate by durable event ID, then continue with live broadcast.
- Cancellation: `CancellationToken` for hub shutdown

## Acceptance Criteria

- [ ] Paired devices can connect via WebSocket
- [ ] Events broadcast to all connected clients
- [ ] Keepalive ping/pong works (dead clients removed)
- [ ] Reconnection sync: missing events replayed on reconnect
- [ ] Unpaired devices rejected at handshake
- [ ] Hub shutdown drains all connections
- [ ] Slow/lagged clients resynchronize without silent event loss
- [ ] Replay-to-live handoff cannot omit or duplicate durable event IDs
- [ ] `cargo test sync` passes
