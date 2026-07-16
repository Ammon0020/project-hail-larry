//! Local Agent Interface — Rust backend (port of the Go daemon).
//!
//! Single Cargo package with focused modules mirroring the Go `internal/`
//! layout. See `docs/plans/rust-port/epic.md` for the architecture decisions
//! and `docs/rust-ecosystem/` for crate selection rationale.
//!
//! Module map (Go `internal/` → Rust `src/`):
//! - [`app`]        — daemon lifecycle, TLS, logging, rate limit (Go `daemon/` + cross-cutting host concerns)
//! - [`acp`]        — Agent Client Protocol client (Go `acp/`)
//! - [`api`]        — REST API handlers (Go `server/api.go`)
//! - [`config`]     — `~/.local-agent/` config storage (Go `config/`)
//! - [`events`]     — SQLite event store, WAL, append-only (Go `events/`)
//! - [`files`]      — revision tracking, three-way merge (Go `files/`)
//! - [`fsutil`]     — shared home-dir + durable atomic write (used by config/MCP/logging)
//! - [`pairing`]    — QR + mnemonic pairing, device credentials (Go `pairing/`)
//! - [`permissions`]— permission request/response, policies (Go `permissions/`)
//! - [`pathutil`]   — path traversal + symlink containment (Go `pathutil/` + `workspace.safeJoin`)
//! - [`search`]     — workspace content search (Go `search/`)
//! - [`shell`]      — workspace-scoped subprocess runner (Go `shell/`)
//! - [`sync`]       — WebSocket hub, broadcast, reconnection (Go `sync/`)
//! - [`workspace`]  — registration, file tree, git info (Go `workspace/`)
//! - [`interfaces`] — shared traits and typed errors (Go `interfaces/`)
//!
//! S-ARCH scope: package layout, MSRV, pinned deps, TLS provider, rate limit
//! stub, file logging stub. Service implementations land in later stories.

// Test code may use `.unwrap()`/`.expect()`/`panic!()` for fail-fast assertions.
// The crate-level `[lints.clippy]` policy denies these in non-test code so the
// daemon cannot accidentally panic on the LAN; this cfg_attr lifts that bar only
// under `cfg(test)`.
#![cfg_attr(test, allow(clippy::unwrap_used, clippy::expect_used, clippy::panic))]

pub mod acp;
pub mod api;
pub mod app;
pub mod config;
pub mod events;
pub mod files;
pub mod fsutil;
pub mod interfaces;
pub mod pairing;
pub mod pathutil;
pub mod permissions;
pub mod search;
pub mod shell;
pub mod sync;
pub mod workspace;
