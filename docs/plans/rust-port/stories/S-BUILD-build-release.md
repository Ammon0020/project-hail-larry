# Story S-BUILD: Build Scripts, Embed, Cross-Compilation

> **Phase:** 5 | **Depends on:** S-CLI | **Go source:** `build.sh`, `build.ps1`

## Summary

Replace Go build scripts with cargo-based build. Handle frontend embed
(`rust-embed`), cross-compilation for Windows/macOS/Linux releases, and
the `cc` requirement for bundled SQLite.

## Current Build Flow

1. `cd web/ && npm install && npm run build` → outputs `internal/server/dist/`
2. `go build` → embeds `dist/` via `go:embed`, compiles binary

## Rust Build Flow

1. `cd web/ && npm install && npm run build` → outputs `internal/server/dist/` (unchanged)
2. `cargo build --release` → `rust-embed` macro embeds `dist/`, compiles binary

### Build scripts

- `build.sh` (Linux/macOS): `npm run build` in `web/`, then `cargo build --release`
- `build.ps1` (Windows): same, PowerShell syntax
- Optional `build.rs`: assert `internal/server/dist/` exists, fail with
  clear "run npm run build first" message if missing

### Cross-compilation

- Linux → Linux: trivial (`cargo build --target x86_64-unknown-linux-gnu`)
- Linux → Windows: `rustup target add x86_64-pc-windows-msvc` + `cargo-xwin`
  or `cross` (Docker-based, handles linker)
- Linux → macOS: `rustup target add aarch64-apple-darwin` + osxcross (or
  build on macOS CI runner)
- CI: use GitHub Actions matrix with native runners per platform

### CGO / C compiler

`rusqlite` "bundled" feature compiles SQLite C source → needs `cc`/`gcc`
at build time. Document this in README. For Windows MSVC, needs the
`cc` crate which finds MSVC build tools automatically.

## Acceptance Criteria

- [ ] `./build.sh` produces working binary on Linux
- [ ] `.\build.ps1` produces working binary on Windows
- [ ] Frontend assets embedded (binary serves UI without `dist/` on disk)
- [ ] Cross-compilation: Linux → Windows binary works
- [ ] Cross-compilation: Linux → macOS binary works (or CI on macOS)
- [ ] Release binaries stripped (`strip = true` in `[profile.release]`)
- [ ] `cc` requirement documented
