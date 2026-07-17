//! Daemon host concerns: TLS provider, file logging, rate limiting, lifecycle.
//!
//! Mirrors Go `internal/daemon/` plus cross-cutting server concerns that don't
//! belong to a single service module. Real lifecycle wiring lands in S-DAEMON.

pub mod listen;
pub mod logging;
pub mod rate_limit;
pub mod tls;
