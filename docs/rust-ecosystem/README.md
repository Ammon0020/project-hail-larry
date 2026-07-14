# Rust Ecosystem Reference

> Library mapping and reference cards for the Go → Rust backend port.
> Each card covers API surface, migration notes, and gotchas for one library
> or library group. Fetch live docs via `context7` during implementation.

## Go → Rust Dependency Mapping

| Go dependency | Rust crate(s) | Confidence | Notes |
|---|---|---|---|
| `coder/acp-go-sdk` | `agent_client_protocol` (official `agentclientprotocol/rust-sdk`) | High | Official SDK, different API surface — see [acp-rust-sdk.md](acp-rust-sdk.md) |
| `net/http` (server) | `axum` + `tokio` + `tower` / `tower-http` | High | See [web-framework.md](web-framework.md) |
| `nhooyr.io/websocket` | `axum::extract::ws` (wraps `tokio-tungstenite`) | High | Built into axum; no separate dep |
| `modernc.org/sqlite` | `rusqlite` (bundled) or `sqlx` (async, `sqlite` feature) | High | See [data-and-concurrency.md](data-and-concurrency.md) |
| `spf13/cobra` | `clap` (derive) | High | See [cli-and-config.md](cli-and-config.md) |
| `fsnotify/fsnotify` | `notify` | High | Cross-platform file watcher |
| `pelletier/go-toml/v2` | `toml` + `serde` | High | Serde derives replace struct tags |
| `skip2/go-qrcode` | `qrcode` | High | PNG output |
| `golang.org/x/time/rate` | `governor` or `tower::limit::RateLimit` | High | Tower middleware integrates with axum |
| `google/uuid` | `uuid` | High | |
| `embed.FS` (`go:embed`) | `rust-embed` | High | See [build-and-embed.md](build-and-embed.md) |
| `log/slog` | `tracing` + `tracing-subscriber` | High | Structured, async-aware |
| `os/exec` | `tokio::process::Command` | High | Async subprocess |
| `crypto/rand`, `crypto/sha256` | `rand`, `sha2` | High | |
| `encoding/json` | `serde` + `serde_json` | High | |
| `container/list` (LRU) | `lru` crate | High | See [data-and-concurrency.md](data-and-concurrency.md) |
| `sync` (Mutex, Once) | `std::sync` / `tokio::sync` | High | See [data-and-concurrency.md](data-and-concurrency.md) |
| `context.Context` | `tokio_util::sync::CancellationToken` + task abort | High | See [data-and-concurrency.md](data-and-concurrency.md) |

## Reference Cards

- [acp-rust-sdk.md](acp-rust-sdk.md) — Agent Client Protocol Rust SDK (the critical dependency)
- [web-framework.md](web-framework.md) — axum, WebSocket, middleware, TLS
- [data-and-concurrency.md](data-and-concurrency.md) — SQLite, async runtime, channels, LRU, cancellation
- [cli-and-config.md](cli-and-config.md) — clap CLI, TOML/JSON config, QR codes
- [build-and-embed.md](build-and-embed.md) — rust-embed, build scripts, cross-compilation, platform services

## Key Architectural Mappings

| Go concept | Rust equivalent |
|---|---|
| `interface` | `trait` + `dyn Trait` or generics |
| `goroutine` | `tokio::spawn` (async task) or `std::thread::spawn` |
| `chan T` | `tokio::sync::mpsc` / `broadcast` / `oneshot` |
| `sync.Mutex` | `std::sync::Mutex` (sync) or `tokio::sync::Mutex` (async) |
| `context.Context` cancellation | `CancellationToken` / task `abort()` |
| `error` wrapping (`%w`) | `anyhow::Context` / `thiserror` for typed errors |
| `defer` | `Drop` impls / RAII guards |
| `go:embed` | `rust-embed` proc macro |
| struct tags (`json:"field"`) | `#[serde(rename = "field")]` / `#[serde(skip_serializing_if)]` |
| `*sql.DB` connection pool | `rusqlite::Connection` (single) or `sqlx::SqlitePool` |
| `http.ServeMux` pattern routing | `axum::Router` with `route()` + method handlers |
| `http.HandlerFunc` | `async fn` handler returning `impl IntoResponse` |

## CGO Note

The Go build uses `modernc.org/sqlite` (pure-Go, no CGO). The standard Rust
SQLite stack (`rusqlite` / `sqlx`) links `libsqlite3-sys` which bundles SQLite
C source and requires a C compiler at build time. This is acceptable for a
self-hosted daemon (not a cross-compile-to-WASM scenario), but it means the
Rust build needs `cc` available. Pure-Rust SQLite alternatives (`limbo`,
`turso`) are immature as of 2026-07 and not recommended for production use yet.
