# No per-IP or per-connection rate limiting on the WebSocket endpoint

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/sync/mod.rs`
- **Lines:** 197-199, 351-397

## Description

`into_router` (line 197-199) registers `GET /ws` with no rate-limit layer, no max-connections-per-IP, and no per-IP subscriber cap. The TCP accept loops (`listen.rs:131-141, 158-184`) spawn an unbounded `JoinSet` task per accepted connection with no global or per-IP ceiling (grep for `max_connections|connection_limit|per_ip` returns zero matches). An attacker with one valid device credential (or any local process via loopback bypass) can open thousands of WS connections, each registering a `ClientEntry` with a 64-slot mpsc channel (`sync/mod.rs:207`, `CLIENT_SEND_CAPACITY=64`). This exhausts file descriptors, memory, and broadcast CPU (every `broadcast` iterates all clients and clones the payload per client). The keepalive ping/pong (30s/10s) eventually reaps idle connections, but an attacker that sends one byte per 40s keeps them alive indefinitely.

## Recommendation

Add a per-IP connection cap (e.g., `DashMap<IpAddr, AtomicU32>` with a configurable max) enforced in `handle_ws` before upgrade, and a global max-clients guard. Apply the same `require_pair_rate_limit`-style middleware to the WS route for unauthenticated connection storms.

## Verification

`into_router` (line 198) — `Router::new().route("/ws", get(handle_ws)).with_state(self)` — no `.layer(...)` for rate limiting. `handle_ws` (line 352-397) has no connection-count check. `register` (line 205-216) unconditionally inserts. grep for `max_connections|connection_limit|per_ip|conn_limit|MAX_CONN` across `src/` — no matches.
