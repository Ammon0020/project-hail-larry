//! WebSocket sync hub (Go `internal/sync/`).
//!
//! `tokio::broadcast`-based broadcast hub with reconnection replay (exp.
//! backoff + jitter), keepalive ping/pong, and loopback auth bypass.
//! Implementation lands in S-SYNC (Phase 4). S-ARCH scope: module
//! placeholder only.
