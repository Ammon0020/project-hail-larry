# Building Local Agent

## Quick start

```bash
./build.sh          # Linux/macOS — frontend + Rust release → bin/local_agent
.\build.ps1         # Windows — same → bin\local_agent.exe
```

Primary binary:

- `bin/local_agent` / `bin/local_agent.exe` — Rust daemon (`local_agent start`)

Frontend-only then Rust:

```bash
cd web && npm run build   # writes web/dist/
cargo build --release     # embeds web/dist/ via rust-embed → target/release/local_agent
```

`build.rs` fails the Rust build if `web/dist/index.html` is missing.

## Release profile (LTO + mold)

Release builds use fat LTO and `codegen-units = 1` (see `[profile.release]` in
`Cargo.toml`) for maximum runtime performance. The first cold release compile
is slower (~6 min) because LTO runs whole-program optimization across all
crates after compilation. Incremental rebuilds are much faster — sccache
caches dependency crates, and only the changed crate + final LTO/link step
re-run.

On x86_64 Linux, [mold](https://github.com/rui314/mold) is used as the linker
(configured in `.cargo/config.toml` via `clang -fuse-ld=mold`) to offset LTO
link time. `./scripts/setup.sh --verify` checks for mold and clang. Other
platforms use the default linker and are unaffected.

`make check` and `make qcheck` use the debug profile, which is unaffected by
LTO. The dev profile uses `debug = "line-tables-only"` for faster compilation
while preserving backtraces.

## C compiler (SQLite)

The Rust event store uses `rusqlite` with the **`bundled`** feature: SQLite C
sources are compiled at build time. You need a C toolchain on `PATH`:

| Platform | Typical requirement |
|----------|---------------------|
| Linux | `gcc` or `clang` (`build-essential` / `clang`) |
| macOS | Xcode Command Line Tools (`xcode-select --install`) |
| Windows | MSVC Build Tools (Visual Studio) — the `cc` crate finds them |

No SQLite shared library is required at runtime; only the compile-time `cc`.
See also [docs/rust-ecosystem/README.md](../rust-ecosystem/README.md) (CGO note).

## Smoke check (embedded SPA)

```bash
local_agent start --background
# open http://127.0.0.1:7337/
local_agent stop
```
