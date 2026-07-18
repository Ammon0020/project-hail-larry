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

1. `cd web/ && npm install && npm run build` → outputs `web/dist/`
2. `cargo build --release` → `rust-embed` embeds `web/dist/`, compiles binary
   (`build.rs` requires `web/dist/index.html`)

Go still copies into `internal/server/dist/` for `go:embed`; Rust does not use
that path.

### Build scripts

- `build.sh` (Linux/macOS): `npm run build` in `web/`, Go build, then
  `cargo build --release` → `bin/local_agent`
- `build.ps1` (Windows): same, PowerShell syntax
- Required `build.rs`: assert `web/dist/index.html` exists and fail with a
  clear "run npm run build / ./build.sh" message if missing.
- Pin supported Rust/toolchain and dependency versions after S-ARCH, commit the
  lockfile, and record dependency-age/security review in release CI.

### Cross-compilation

- Linux → Linux: trivial (`cargo build --target x86_64-unknown-linux-gnu`)
- Windows release artifacts build and test on native Windows CI; local
  cross-compilation is a contributor convenience, not the release guarantee.
- macOS: build on native macOS CI runners; define universal-binary and
  signing/notarization policy rather than relying on Linux cross-compilation.
- CI: use native Linux, macOS, and Windows runners for release artifacts.

### CGO / C compiler

`rusqlite` "bundled" feature compiles SQLite C source → needs `cc`/`gcc`
at build time. Documented in `docs/development/building.md`. For Windows MSVC,
the `cc` crate finds MSVC build tools automatically.

## Acceptance Criteria

- [x] `./build.sh` produces working binary on Linux
- [x] `.\build.ps1` produces working binary on Windows
- [x] Frontend assets embedded (binary serves UI without `dist/` on disk)
- [ ] Windows release artifact builds and passes smoke tests on native Windows CI
- [ ] macOS release artifact builds and passes smoke tests on native macOS CI
- [x] Required build.rs asset check fails clearly when frontend dist is missing
- [x] Release binaries stripped (`strip = true` in `[profile.release]`)
- [x] `cc` requirement documented

> **Deferred (STATUS):** Native Win/macOS *release artifact + SPA smoke* CI.
> `rust-ci.yml` already runs basic `cargo build`/`test` on those runners with a
> stub `web/dist/index.html` so `build.rs` is satisfied without Node.
