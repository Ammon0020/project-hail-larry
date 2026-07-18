# Building Local Agent

## Quick start

```bash
./build.sh          # Linux/macOS — frontend + Rust release → bin/local_agent
.\build.ps1         # Windows — same → bin\local_agent.exe
```

Primary binary:

- `bin/local_agent` / `bin/local_agent.exe` — Rust daemon (`local_agent start`)

Optional legacy Go binary (oracle / rollback):

```bash
BUILD_GO=1 ./build.sh          # also builds bin/app
$env:BUILD_GO=1; .\build.ps1   # Windows
```

Frontend-only then Rust:

```bash
cd web && npm run build   # writes web/dist/
cargo build --release     # embeds web/dist/ via rust-embed → target/release/local_agent
```

`build.rs` fails the Rust build if `web/dist/index.html` is missing. Go’s
`internal/server/dist/` is only used when `BUILD_GO=1`.

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
