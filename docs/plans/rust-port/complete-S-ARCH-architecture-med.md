# Story S-ARCH: Architecture and Dependency Decisions

> **Phase:** 0 | **Depends on:** — | **Go source:** cross-cutting

## Goal

Freeze the implementation choices that affect every Rust module before code is
written. This story produces decisions and small compile-only validation, not a
backend implementation.

## Scope

- Single Cargo package with focused modules and a documented MSRV
- Current, compatible ACP SDK crate/API selection
- `rusqlite` blocking-boundary design and persisted-state compatibility policy
- Rustls crypto provider, TLS serving approach, and governor-based rate limit
- Stable file logging location and native-platform release CI strategy
- Minimal, pinned dependency set with lockfile and dependency-age review

## Acceptance Criteria

- [x] Cargo package layout and MSRV are documented and compile in CI
- [x] Every crate choice has a current-docs verification date and a reason
- [x] Exactly one rustls crypto provider is selected and tested at startup
- [x] Rate limiting uses a supported tower-compatible governor integration
- [x] File logs have a stable `~/.local-agent/logs/` location
- [x] macOS releases build on native macOS CI; Windows releases build/test on Windows CI
- [x] No dependency is adopted solely because it mirrors a Go implementation

## Completion Notes (2026-07-15)

- **Package layout:** single Cargo package at repo root; `src/{app,acp,api,
  config,events,files,pairing,permissions,search,shell,sync,workspace,
  interfaces}/mod.rs` mirrors Go `internal/`. `src/app/` absorbs Go
  `internal/daemon/` plus cross-cutting host concerns (TLS, logging, rate
  limit). MSRV `1.92.0` declared in `Cargo.toml` and pinned in
  `rust-toolchain.toml`.
- **Pinned deps:** every dependency in `Cargo.toml` carries a verification
  date (2026-07-15) and a reason comment. Versions resolved via `cargo fetch`
  against the ecosystem docs. `Cargo.lock` committed (application binary).
- **TLS provider:** `aws-lc-rs` selected over `ring` (rustls default, FIPS
  path, AWS-audited). `src/app/tls.rs::install_crypto_provider()` installs it
  process-wide; `crypto_provider_installs_at_startup` test verifies.
- **Rate limiting:** `tower_governor` (wraps `governor` as a `tower::Layer`),
  keyed by peer IP. `src/app/rate_limit.rs` exposes the builder shape; exact
  Go-equivalent burst/period values deferred to S-SERVER after S-CONTRACT
  captures them.
- **File logging:** `tracing-appender` non-blocking daily roller at
  `~/.local-agent/logs/local-agent.log` (`src/app/logging.rs`).
- **CI:** `.github/workflows/rust-ci.yml` — fmt (Linux), clippy `-D warnings`
  (Linux), test (Linux), build (macOS), build+test (Windows). Uses
  `actions/checkout@v4` + `dtolnay/rust-toolchain@stable`.
- **Verification:** `cargo fmt --check`, `cargo clippy -- -D warnings`,
  `cargo test` (6 passed), `cargo build` all clean on stable 1.92.0.
- **Deferred to later stories:** ACP Rust SDK crate (`agent_client_protocol`)
  is not yet added — S-ACP-SPIKE verifies the current API surface before it is
  pinned. `rust-embed`, `notify`, `qrcode`, `lru`, `uuid`, `rand`, `sha2`,
  `rcgen` are added by the stories that first need them (S-BUILD, S-FSWATCH,
  S-PAIRING, S-FILES, S-INTERFACES respectively) to keep the S-ARCH dep set
  minimal.
