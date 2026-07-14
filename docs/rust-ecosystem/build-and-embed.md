# Build & Embed Reference

> `rust-embed` (frontend assets), build scripts, platform service install,
> cross-compilation.

## Embedding Frontend (replaces `go:embed all:dist`)

```rust
use rust_embed::RustEmbed;

#[derive(RustEmbed)]
#[folder = "internal/server/dist/"]
struct FrontendAsset;

// Usage:
match FrontendAsset::get("index.html") {
    Some(content) => /* serve bytes */,
    None => /* 404 or SPA fallback */,
}
```

`rust-embed` embeds files at compile time, same as `go:embed`. The
`internal/server/dist/` directory (populated by `npm run build` in `web/`)
remains the source — only the embedding mechanism changes.

### Build script coordination

The current build flow: `build.sh` runs `npm run build` (outputs to
`internal/server/dist/`), then `go build` embeds it. In Rust:

1. `npm run build` → outputs to `internal/server/dist/` (unchanged)
2. `cargo build` → `rust-embed` macro reads `dist/` at compile time

The `build.sh` / `build.ps1` scripts change the final step from `go build`
to `cargo build`. A `build.rs` script is not needed unless you want to
assert `dist/` exists and fail early with a clear message.

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

## Cargo.toml Dependencies (starting point)

```toml
[dependencies]
# Async runtime
tokio = { version = "1", features = ["full"] }
tokio-util = { version = "0.7", features = ["rt"] }
futures-util = "0.3"

# Web framework
axum = { version = "0.8", features = ["ws"] }
axum-server = { version = "0.7", features = ["tls-rustls"] }
tower = "0.5"
tower-http = { version = "0.6", features = ["fs", "cors", "limit"] }

# ACP
agent_client_protocol = "0.1"  # verify latest from crates.io

# Database
rusqlite = { version = "0.32", features = ["bundled"] }

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"
toml = "0.8"

# CLI
clap = { version = "4", features = ["derive"] }

# Embed
rust-embed = "8"

# Utilities
uuid = { version = "1", features = ["v4"] }
notify = "6"
qrcode = "0.14"
lru = "0.12"
rand = "0.8"
sha2 = "0.10"
hex = "0.4"
base64 = "0.22"
regex = "1"

# Error handling
anyhow = "1"
thiserror = "1"

# Logging
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
```

> Verify latest versions on crates.io before pinning. Prefer versions
> published at least 7 days ago (per AGENTS.md dependency policy).
