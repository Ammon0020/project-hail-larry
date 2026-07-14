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

- [ ] Cargo package layout and MSRV are documented and compile in CI
- [ ] Every crate choice has a current-docs verification date and a reason
- [ ] Exactly one rustls crypto provider is selected and tested at startup
- [ ] Rate limiting uses a supported tower-compatible governor integration
- [ ] File logs have a stable `~/.local-agent/logs/` location
- [ ] macOS releases build on native macOS CI; Windows releases build/test on Windows CI
- [ ] No dependency is adopted solely because it mirrors a Go implementation
