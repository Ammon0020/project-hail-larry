# Building Local Agent

## Quick start

```bash
./build.sh          # Linux/macOS — frontend + Rust release → bin/local_agent
.\build.ps1         # Windows — same → bin\local_agent.exe
```

## Dev mode (HMR)

```bash
make dev            # or: scripts/dev.sh
```

Starts the Rust daemon (`cargo run -- start`) and the Vite dev server
(`npm run dev`) together. Open **http://localhost:5173** — Vite proxies
`/api`, `/ws`, and `/health` to the daemon on port 7337. You get instant
frontend HMR.

If [cargo-watch](https://github.com/watchexec/cargo-watch) is installed,
the daemon also auto-rebuilds and restarts on changes under `src/`,
`Cargo.toml`, `build.rs`, `rust-toolchain.toml`, and `configs/` — no
manual restart needed for Rust edits. Install it with
`cargo install cargo-watch` (or run `./scripts/setup.sh`, which installs
it as an optional dev convenience). Without cargo-watch, Rust changes
require a manual restart (Ctrl+C, then `make dev` again).

Requires `web/dist/index.html` to exist (build.rs needs it for `cargo run`
to compile). It persists across builds; run `cd web && npm run build` once
only if you've run `make clean`.

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
to offset LTO link time. sccache and mold are local-dev optimizations only —
the repo's `.cargo/config.toml` is intentionally empty so CI runners without
these tools build out of the box. `./scripts/setup.sh` writes the sccache +
mold config to the user-level `~/.cargo/config.toml`; `./scripts/setup.sh
--verify` checks for the tools. Other platforms use the default linker and
are unaffected.

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
