# Building Local Agent

## Quick start

```bash
./build.sh          # Linux/macOS — frontend + Go + Rust release
.\build.ps1         # Windows — same
```

Outputs:

- `bin/app` / `bin/app.exe` — Go daemon (`app start`)
- `bin/local_agent` / `bin/local_agent.exe` — Rust daemon (`local_agent start`)

Frontend-only then Rust:

```bash
cd web && npm run build   # writes web/dist/
cargo build --release     # embeds web/dist/ via rust-embed → target/release/local_agent
```

`build.rs` fails the Rust build if `web/dist/index.html` is missing. Go’s
`internal/server/dist/` is only for the Go binary; the Rust port does not use it.

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

After `cd web && npm run build` and `cargo build --release`, the binary embeds
`index.html`. Unit coverage: `src/api/embed.rs` (`spa_entry_serves_html_or_clear_build_fallback`).
Manual: `strings target/release/local_agent | grep -F '<!doctype html>'` (or start the
daemon and open `/`).
