# Build & Embed Reference

> `rust-embed` (frontend assets), build scripts, platform service install,
> cross-compilation.

## Embedding Frontend (replaces `go:embed all:dist`)

```rust
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "$CARGO_MANIFEST_DIR/web/dist/"]
struct FrontendAsset;

// Usage:
match FrontendAsset::get("index.html") {
    Some(content) => /* serve bytes */,
    None => /* 404 or SPA fallback */,
}
```

`rust-embed` embeds files at compile time. Vite writes the production SPA to
**`web/dist/`**; that directory is the Rust embed root.

### Build script coordination

1. `cd web && npm run build` → outputs to `web/dist/`
2. `cargo build --release` → `rust-embed` reads `web/dist/` at compile time;
   `build.rs` fails early if `web/dist/index.html` is missing

`./build.sh` / `.\build.ps1` run the frontend build, then Go, then
`cargo build --release`. See [docs/development/building.md](../development/building.md)
for the `cc` / bundled-SQLite requirement.

### C compiler for bundled SQLite

`rusqlite` with `bundled` compiles SQLite C source at build time — a C toolchain
(`gcc`/`clang`/MSVC) must be on `PATH`. Documented in
[docs/development/building.md](../development/building.md).

## Error Handling — `anyhow` + `thiserror`

```rust
// Application-level errors (anyhow for propagation, thiserror for typed)
use thiserror::Error;

#[derive(Error, Debug)]
enum AppError {
    #[error("session not found: {0}")]
    SessionNotFound(String),
    #[error("stale revision: file modified since last read")]
    StaleRevision,
    #[error("workspace not found: {0}")]
    WorkspaceNotFound(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Db(#[from] rusqlite::Error),
}
```

Go's `fmt.Errorf("...: %w", err)` → `anyhow::Context::context()` for
ad-hoc wrapping, or `#[from]` derives for typed conversion.

## Logging — `tracing` (replaces `log/slog`)

```rust
use tracing::{info, warn, error, instrument};

#[instrument(skip(self))]
async fn create_session(&self, agent_id: &str) -> Result<Session, AppError> {
    info!(agent_id, "creating session");
    // ...
}
```

`tracing-subscriber` for formatting. Structured fields replace slog's
key-value pairs. `#[instrument]` auto-logs function entry/exit with args.
Use `tracing-appender` (or an equivalently reviewed non-blocking writer) for
rolling files at `~/.local-agent/logs/`; redact credentials, passcodes, bearer
tokens, and file contents.

## Platform Service Install (replaces Go build tags)

Go uses `service_linux.go` / `service_darwin.go` / `service_windows.go`
with build tags. Rust uses `#[cfg(target_os)]`:

```rust
#[cfg(target_os = "linux")]
mod service {
    // systemd unit generation, install, uninstall
}

#[cfg(target_os = "macos")]
mod service {
    // launchd plist generation, install, uninstall
}

#[cfg(target_os = "windows")]
mod service {
    // HKCU registry entry, Windows service install
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
mod service {
    pub fn install() -> Result<()> { anyhow::bail!("unsupported platform") }
}
```

Platform-specific daemon stop logic (`stop_unix.go` sends SIGTERM,
`stop_windows.go` uses taskkill) maps the same way.

## Cross-Compilation

The Go build cross-compiles easily (`GOOS=windows go build`). Rust needs
target triples installed via `rustup target add` and potentially a cross
linker. For Windows-from-Linux, `cargo-xwin` or `cross` (Docker-based)
simplifies this. This is a build-tooling task, not a code change.

## Cargo.toml Dependencies (decision checklist)

Do not copy an unverified version list into `Cargo.toml`. S-ARCH creates the
pinned manifest and lockfile after checking current documentation and the
repository minimum-release-age policy. It must include only required crates:

- Tokio, tokio-util, axum, tower, tower-http, and one reviewed TLS serving path
- the current official ACP core plus its verified Tokio/process and MCP helpers
- rusqlite with bundled SQLite, serde/serde_json/toml, clap, and rust-embed
- governor-based rate limiting, rcgen, tracing-appender, and the chosen rustls
  crypto provider
- `dirs` + `atomic-write-file` (shared via `src/fsutil` for home + durable
  state writes)
- notify plus a maintained debouncer, lru, similar, ignore, infer, subtle,
  dashmap 6.x, and the image support needed by QR PNG rendering
- search: prefer shell-out to `rg --json` with `ignore`/`regex` fallback
  (matches Go); do not pure-Rust reimplement ripgrep first
- platform-gated service/process dependencies only where implementation needs
  them; avoid `tower-http` filesystem serving when rust-embed owns static files

Record each selected version, MSRV impact, license/security review, and the
reason it is needed in the S-ARCH completion notes. The committed lockfile is
the reproducible source of truth.
