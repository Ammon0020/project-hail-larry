# Data & Concurrency Reference

> SQLite, async runtime, channels, LRU cache, cancellation, file watching.

## SQLite (replaces `modernc.org/sqlite` + `database/sql`)

### Option A: `rusqlite` (sync, bundled) — recommended for the event store

```rust
use rusqlite::Connection;

let conn = Connection::open(db_path)?;
conn.pragma_update(None, "journal_mode", "WAL")?;
conn.pragma_update(None, "busy_timeout", 5000)?;
```

The Go code uses `MaxOpenConns(1)` to serialize writes. In Rust, put the
single connection behind a dedicated blocking database boundary and run DB ops
in `tokio::task::spawn_blocking`; callers must never hold a mutex across
`.await`. This preserves write ordering while avoiding async-runtime blocking:

```rust
let db = Arc::new(Mutex::new(conn));
// In async context:
let db = db.clone();
tokio::task::spawn_blocking(move || {
    let conn = db.lock().unwrap();
    conn.execute("INSERT INTO events ...", params![...])
}).await?;
```

The `rusqlite` "bundled" feature compiles SQLite C source — needs `cc` at
build time but no runtime SQLite dependency.

### Option B: `sqlx` (async, compile-time checked queries)

```rust
use sqlx::sqlite::SqlitePoolOptions;

let pool = SqlitePoolOptions::new()
    .max_connections(1)
    .connect("sqlite://events.db?mode=rwc").await?;
sqlx::query("PRAGMA journal_mode=WAL").execute(&pool).await?;
```

`sqlx::query!` macros check SQL at compile time against the schema. Heavier
dependency but more ergonomic if queries are complex. The event store's
append/query pattern is simple enough that `rusqlite` + `spawn_blocking`
is sufficient and lighter.

**Recommendation:** `rusqlite` (bundled) for the event store. It's the
closest 1:1 mapping to the current `database/sql` usage and avoids pulling
in the full sqlx macro machinery.

## Async Runtime — `tokio`

```rust
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // daemon setup...
    Ok(())
}
```

Multi-threaded runtime (default) maps to Go's GOMAXPROCS scheduler. Use
`tokio::spawn` for background tasks (file watcher, keepalive, event
broadcast) — these are the goroutines in the Go code.

## Channels (replaces Go `chan`)

| Go | Rust |
|---|---|
| `chan T` (unbuffered) | `tokio::sync::mpsc::channel(1)` |
| `chan T` (buffered) | `tokio::sync::mpsc::channel(n)` |
| broadcast to many | `tokio::sync::broadcast::channel(n)` |
| one-shot response | `tokio::sync::oneshot::channel()` |

The sync hub's broadcast pattern maps to `tokio::sync::broadcast` — each
WebSocket client subscribes to a receiver, the hub sends events to all.

## Mutex (replaces `sync.Mutex` / `sync.RWMutex`)

- `std::sync::Mutex` — for short critical sections with no await inside
- `tokio::sync::Mutex` — when the lock must be held across `.await` points
- `std::sync::RwLock` / `tokio::sync::RwLock` — read-heavy patterns
- `parking_lot::Mutex` — faster, fairer alternative to `std::sync::Mutex`

The Go code's per-file locks (`files.go`) and the ACP client's
`termMu`/session locks map to `tokio::sync::Mutex` (held across I/O).

## LRU Cache (replaces `container/list` hand-rolled LRU)

```rust
use lru::LruCache;
let mut cache: LruCache<String, Vec<u8>> = LruCache::new(NonZeroUsize::new(256).unwrap());
```

The `files.go` `lruCache` (256-entry base-content cache for three-way merge)
maps directly to the `lru` crate.

## Cancellation (replaces `context.Context`)

```rust
use tokio_util::sync::CancellationToken;

let token = CancellationToken::new();
// In tasks:
tokio::select! {
    _ = token.cancelled() => break,
    _ = do_work() => {},
}
// To cancel: token.cancel();
```

The daemon's lifecycle context (cancelled on shutdown, derived by all
goroutines) maps to a `CancellationToken` shared via `Arc`. For per-session
cancellation, each session gets its own token.

## File Watching (replaces `fsnotify/fsnotify`)

```rust
use notify::{Watcher, RecursiveMode, EventKind};

let mut watcher = notify::recommended_watcher(|res: Result<Event, _>| {
    match res {
        Ok(event) => match event.kind {
            EventKind::Modify(_) => { /* emit FileChangedOnDisk */ },
            _ => {}
        },
        Err(e) => tracing::error!("watch error: {e}"),
    }
})?;

watcher.watch(path, RecursiveMode::Recursive)?;
```

The `internal/fswatch` package maps cleanly — `notify` is the standard Rust
cross-platform watcher. Prefer its maintained debouncer companion over a
hand-rolled sleep loop. Preserve the current suppression and event semantics
through contract tests before adopting additional watcher behavior.

## Fetching Live Docs

```
context7: resolve-library-id "rusqlite"
context7: resolve-library-id "notify rust file watcher"
context7: resolve-library-id "tokio"
```
