//! Local Agent Interface — Rust backend (port of the Go daemon).
//!
//! Single Cargo package with focused modules mirroring the Go `internal/`
//! layout. See `docs/plans/active-rust-port-hard.md` for the architecture decisions
//! and `docs/rust-ecosystem/` for crate selection rationale.
//!
//! Module map (Go `internal/` → Rust `src/`):
//! - [`app`]        — daemon lifecycle, TLS, logging, rate limit (Go `daemon/` + cross-cutting host concerns)
//! - [`acp`]        — Agent Client Protocol client (Go `acp/`)
//! - [`api`]        — REST API handlers (Go `server/api.go`)
//! - [`config`]     — `~/.local-agent/` config storage (Go `config/`)
//! - [`events`]     — SQLite event store, WAL, append-only (Go `events/`)
//! - [`files`]      — revision tracking, per-file locking, LRU base-content cache (Go `files/`)
//! - [`fsutil`]     — shared home-dir + durable atomic write (used by config/MCP/logging)
//! - [`fswatch`]    — on-disk change detection, external file changes (Go `fswatch/`)
//! - [`pairing`]    — QR + mnemonic pairing, device credentials (Go `pairing/`)
//! - [`permissions`]— permission request/response, policies (Go `permissions/`)
//! - [`pathutil`]   — path traversal + symlink containment (Go `pathutil/` + `workspace.safeJoin`)
//! - [`procutil`]   — Unix process-group setup + kill for shell/ACP subprocess trees
//! - [`search`]     — workspace content search (Go `search/`)
//! - [`shell`]      — workspace-scoped subprocess runner (Go `shell/`)
//! - [`sync`]       — WebSocket hub, broadcast, reconnection (Go `sync/`)
//! - [`uploads`]    — per-session file upload store, magic-byte MIME (Go `uploads/`)
//! - [`workspace`]  — registration, file tree, git info (Go `workspace/`)
//! - [`interfaces`] — shared traits and typed errors (Go `interfaces/`)
//! - [`migrate`]    — Go→Rust state migration + validation (S-MIGRATE)
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
pub mod cli;
pub mod config;
pub mod events;
pub mod files;
pub mod fsutil;
pub mod fswatch;
pub mod interfaces;
pub mod mcp;
pub mod migrate;
pub mod pairing;
pub mod pathutil;
pub mod permissions;
pub mod procutil;
pub mod search;
pub mod shell;
pub mod sync;
pub mod uploads;
pub mod workspace;
