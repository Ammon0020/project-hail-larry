# Story S-SYNC: WebSocket Sync Hub

> **Phase:** 4 | **Depends on:** S-EVENTS | **Go source:** `internal/sync/` (347 lines)

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
- Hub: `DashMap<ClientId, mpsc::Sender<Message>>` for client registry
- Broadcast: `tokio::sync::broadcast::Sender<Event>` — each client
  subscribes to a receiver
- Keepalive: `tokio::time::interval(30s)` ping task per client
- Auth: `AuthChecker` callback (delegates to S-PAIRING) — gate handshake
- Reconnection: client sends last event ID → server replays missing via
  S-EVENTS `Query`/`QueryAll`, then switches to live broadcast
- Cancellation: `CancellationToken` for hub shutdown

## Acceptance Criteria

- [ ] Paired devices can connect via WebSocket
- [ ] Events broadcast to all connected clients
- [ ] Keepalive ping/pong works (dead clients removed)
- [ ] Reconnection sync: missing events replayed on reconnect
- [ ] Unpaired devices rejected at handshake
- [ ] Hub shutdown drains all connections
- [ ] `cargo test sync` passes
