//! Daemon host concerns: lifecycle, listeners, TLS, logging, and rate limiting.
//!
//! Mirrors Go `internal/daemon/` plus cross-cutting server concerns that don't
//! belong to a single service module.

pub mod daemon;
pub mod listen;
pub mod logging;
pub mod process;
pub mod rate_limit;
pub mod tls;
pub mod tls_cert;
