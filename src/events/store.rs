//! SQLite-backed append-only event store (Go `events.Store`).
//!
//! A single `rusqlite::Connection` is owned behind a blocking boundary
//! (`std::sync::Mutex`). Async callers enter via `spawn_blocking` and never hold
//! the DB lock across `.await`. This mirrors Go's `MaxOpenConns(1)`: one
//! connection serializes writes and eliminates SQLITE_BUSY contention between
//! pooled connections.
//!
//! Schema, indexes, PRAGMAs, and payload encoding match
//! `internal/events/events.go` exactly for S-MIGRATE compatibility.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use chrono::{DateTime, NaiveDateTime, Utc};
use rusqlite::{params, Connection};
use tracing::{debug, warn};

use super::payload::{decode_payload, encode_payload};
use crate::interfaces::types::{go_zero_time, Event, EventType};
use crate::interfaces::{AppError, EventStore};

/// How long a contended SQLite write waits before failing (milliseconds).
/// Matches Go `sqliteBusyTimeout`.
const SQLITE_BUSY_TIMEOUT_MS: i32 = 5000;

/// Default row limit when callers pass `limit <= 0`. Matches Go Query/QueryAll.
const DEFAULT_QUERY_LIMIT: i32 = 1000;

/// Default maximum events retained by the background prune ticker.
/// Matches Go `DefaultPruneMaxRows`.
pub const DEFAULT_PRUNE_MAX_ROWS: i64 = 100_000;

/// Default interval between automatic prune passes.
/// Matches Go `DefaultPruneInterval`.
pub const DEFAULT_PRUNE_INTERVAL: Duration = Duration::from_secs(3600);

/// DDL matching Go `events.New` schema creation byte-for-byte in structure.
const SCHEMA_SQL: &str = r"
CREATE TABLE IF NOT EXISTS events (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    type TEXT NOT NULL,
    session_id TEXT NOT NULL,
    timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
    payload TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);
CREATE INDEX IF NOT EXISTS idx_events_session_id_id ON events(session_id, id);
";

/// Append-only SQLite event store.
///
/// Cheap to clone: the connection is shared via `Arc`. All clones serialize
/// through the same mutex-owned connection.
#[derive(Clone)]
pub struct Store {
    /// Single SQLite connection. Locked only inside blocking tasks.
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    /// Open (or create) the event database at `db_path`.
    ///
    /// Sets WAL mode, `busy_timeout=5000`, best-effort incremental auto-vacuum,
    /// and creates the events schema/indexes if missing. Matches Go `events.New`.
    ///
    /// # Errors
    /// Returns [`AppError::Internal`] when the database cannot be opened or
    /// initialized.
    pub fn open(db_path: impl AsRef<Path>) -> Result<Self, AppError> {
        let path = db_path.as_ref();
        let conn = Connection::open(path)
            .map_err(|e| AppError::internal(format!("open sqlite {}: {e}", path.display())))?;

        // Enable WAL mode for append-heavy workloads with concurrent readers.
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(|e| AppError::internal(format!("set WAL mode: {e}")))?;

        // Contended writes wait up to busy_timeout instead of failing immediately.
        conn.pragma_update(None, "busy_timeout", SQLITE_BUSY_TIMEOUT_MS)
            .map_err(|e| AppError::internal(format!("set busy_timeout: {e}")))?;

        // Best-effort: enable incremental auto-vacuum so pruning can reclaim
        // pages. On an existing DB where auto_vacuum was OFF this is a no-op
        // without VACUUM — match Go and ignore the result.
        let _ = conn.pragma_update(None, "auto_vacuum", "INCREMENTAL");

        conn.execute_batch(SCHEMA_SQL)
            .map_err(|e| AppError::internal(format!("create schema: {e}")))?;

        debug!(path = %path.display(), "event store opened");
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    /// Run a blocking closure on the dedicated connection via `spawn_blocking`.
    ///
    /// The closure receives exclusive access to the connection; the lock is
    /// never held across an `.await` in async callers.
    async fn with_conn<T, F>(&self, f: F) -> Result<T, AppError>
    where
        T: Send + 'static,
        F: FnOnce(&Connection) -> Result<T, AppError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let guard = conn.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
            f(&guard)
        })
        .await
        .map_err(|e| AppError::internal(format!("event store blocking task failed: {e}")))?
    }

    /// Append an event synchronously (blocking). Used by unit tests and the
    /// blocking boundary inside async methods.
    fn append_blocking(conn: &Connection, mut event: Event) -> Result<Event, AppError> {
        // Go: if e.Timestamp.IsZero() { e.Timestamp = time.Now().UTC() }
        if event.timestamp == go_zero_time() {
            event.timestamp = Utc::now();
        }

        let payload = encode_payload(&event)
            .map_err(|e| AppError::internal(format!("marshal payload: {e}")))?;
        let type_str = event.event_type.as_str();
        // Store as RFC3339 UTC; scan also accepts the plain SQLite datetime form.
        let ts = event.timestamp.to_rfc3339();

        conn.execute(
            "INSERT INTO events (type, session_id, timestamp, payload) VALUES (?1, ?2, ?3, ?4)",
            params![type_str, event.session_id, ts, payload],
        )
        .map_err(|e| AppError::internal(format!("insert event: {e}")))?;

        let id = conn.last_insert_rowid();
        event.id = id;
        Ok(event)
    }

    /// Query events for a session after `after_id` (blocking).
    fn query_blocking(
        conn: &Connection,
        session_id: &str,
        after_id: i64,
        limit: i32,
    ) -> Result<Vec<Event>, AppError> {
        let limit = if limit <= 0 {
            DEFAULT_QUERY_LIMIT
        } else {
            limit
        };
        let mut stmt = conn
            .prepare(
                "SELECT id, type, session_id, timestamp, payload \
                 FROM events WHERE session_id = ?1 AND id > ?2 \
                 ORDER BY id ASC LIMIT ?3",
            )
            .map_err(|e| AppError::internal(format!("query events: {e}")))?;

        let rows = stmt
            .query_map(params![session_id, after_id, limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| AppError::internal(format!("query events: {e}")))?;

        scan_event_rows(rows)
    }

    /// Query events across all sessions after `after_id` (blocking).
    fn query_all_blocking(
        conn: &Connection,
        after_id: i64,
        limit: i32,
    ) -> Result<Vec<Event>, AppError> {
        let limit = if limit <= 0 {
            DEFAULT_QUERY_LIMIT
        } else {
            limit
        };
        let mut stmt = conn
            .prepare(
                "SELECT id, type, session_id, timestamp, payload \
                 FROM events WHERE id > ?1 ORDER BY id ASC LIMIT ?2",
            )
            .map_err(|e| AppError::internal(format!("query all events: {e}")))?;

        let rows = stmt
            .query_map(params![after_id, limit], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                ))
            })
            .map_err(|e| AppError::internal(format!("query all events: {e}")))?;

        scan_event_rows(rows)
    }

    /// Delete oldest events so at most `max_rows` remain.
    ///
    /// Returns the number of rows deleted. Non-positive `max_rows` is an error
    /// (never silently truncate the whole table). Matches Go `Store.Prune`.
    pub async fn prune(&self, max_rows: i64) -> Result<i64, AppError> {
        if max_rows <= 0 {
            return Err(AppError::internal(format!(
                "events: Prune requires maxRows > 0, got {max_rows}"
            )));
        }
        self.with_conn(move |conn| {
            prune_with_query(
                conn,
                "prune by row count",
                "DELETE FROM events WHERE id NOT IN (SELECT id FROM events ORDER BY id DESC LIMIT ?1)",
                &[&max_rows as &dyn rusqlite::ToSql],
            )
        })
        .await
    }

    /// Delete events older than `max_age` measured from now (UTC).
    ///
    /// Matches Go `Store.PruneOlderThan`.
    pub async fn prune_older_than(&self, max_age: Duration) -> Result<i64, AppError> {
        if max_age.is_zero() {
            return Err(AppError::internal(
                "events: PruneOlderThan requires maxAge > 0, got 0",
            ));
        }
        let cutoff = Utc::now()
            - chrono::Duration::from_std(max_age).map_err(|e| {
                AppError::internal(format!("events: PruneOlderThan duration out of range: {e}"))
            })?;
        let cutoff_str = cutoff.to_rfc3339();
        self.with_conn(move |conn| {
            prune_with_query(
                conn,
                "prune by age",
                "DELETE FROM events WHERE timestamp < ?1",
                &[&cutoff_str as &dyn rusqlite::ToSql],
            )
        })
        .await
    }

    /// Count rows in the events table (test / diagnostic helper).
    pub async fn count(&self) -> Result<i64, AppError> {
        self.with_conn(|conn| {
            conn.query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
                .map_err(|e| AppError::internal(format!("count events: {e}")))
        })
        .await
    }

    /// Read a PRAGMA value as a string (tests: journal_mode / busy_timeout).
    ///
    /// Some PRAGMAs (e.g. `busy_timeout`) are returned as integers by SQLite;
    /// those are stringified so callers get a uniform `String`.
    pub async fn pragma(&self, name: &str) -> Result<String, AppError> {
        let name = name.to_string();
        self.with_conn(move |conn| {
            // Prefer string, fall back to integer (busy_timeout) / other scalars.
            conn.pragma_query_value(None, &name, |row| row.get::<_, String>(0))
                .or_else(|_| {
                    conn.pragma_query_value(None, &name, |row| {
                        let v: i64 = row.get(0)?;
                        Ok(v.to_string())
                    })
                })
                .map_err(|e| AppError::internal(format!("pragma {name}: {e}")))
        })
        .await
    }

    /// Launch a background task that calls [`Self::prune`] every `interval`.
    ///
    /// Returns a stop handle that cancels the ticker and is safe to call multiple
    /// times. Non-positive interval / max_rows fall back to defaults
    /// (`DEFAULT_PRUNE_INTERVAL` / `DEFAULT_PRUNE_MAX_ROWS`). Matches Go
    /// `StartPruneTicker`.
    pub fn start_prune_ticker(
        &self,
        interval: Duration,
        max_rows: i64,
    ) -> tokio::task::JoinHandle<()> {
        let interval = if interval.is_zero() {
            DEFAULT_PRUNE_INTERVAL
        } else {
            interval
        };
        let max_rows = if max_rows <= 0 {
            DEFAULT_PRUNE_MAX_ROWS
        } else {
            max_rows
        };
        let store = self.clone();
        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(interval);
            // First tick completes immediately; skip it so we wait a full interval
            // before the first prune (matches Go ticker semantics).
            ticker.tick().await;
            loop {
                ticker.tick().await;
                // Best-effort: prune errors are non-actionable from the ticker;
                // the next tick retries. Log at warn so operators can notice.
                match store.prune(max_rows).await {
                    Ok(deleted) if deleted > 0 => {
                        debug!(deleted, max_rows, "event store prune ticker deleted rows");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        warn!(error = %e, "event store prune ticker failed");
                    }
                }
            }
        })
    }
}

#[async_trait]
impl EventStore for Store {
    /// Append an event; returns it with the assigned monotonic ID.
    async fn append(&self, event: Event) -> Result<Event, AppError> {
        self.with_conn(move |conn| Self::append_blocking(conn, event))
            .await
    }

    /// Events for `session_id` with `id > after_id`, ordered ascending, limited.
    async fn query(
        &self,
        session_id: &str,
        after_id: i64,
        limit: i32,
    ) -> Result<Vec<Event>, AppError> {
        let session_id = session_id.to_string();
        self.with_conn(move |conn| Self::query_blocking(conn, &session_id, after_id, limit))
            .await
    }

    /// Events across all sessions with `id > after_id`.
    async fn query_all(&self, after_id: i64, limit: i32) -> Result<Vec<Event>, AppError> {
        self.with_conn(move |conn| Self::query_all_blocking(conn, after_id, limit))
            .await
    }
}

/// Convert mapped SQLite rows into `Event` values.
fn scan_event_rows(
    rows: impl Iterator<Item = Result<(i64, String, String, String, String), rusqlite::Error>>,
) -> Result<Vec<Event>, AppError> {
    let mut events = Vec::new();
    for row in rows {
        let (id, type_str, session_id, timestamp_str, payload_str) =
            row.map_err(|e| AppError::internal(format!("scan event: {e}")))?;

        let event_type = parse_event_type(&type_str)?;
        let timestamp = parse_timestamp(&timestamp_str)?;

        let mut event = Event::new(id, event_type, session_id, timestamp);
        decode_payload(&payload_str, &mut event)
            .map_err(|e| AppError::internal(format!("unmarshal payload: {e}")))?;
        events.push(event);
    }
    Ok(events)
}

/// Parse an event type wire string into the enum.
///
/// Unknown types surface as internal errors (corrupt row) rather than panicking.
fn parse_event_type(s: &str) -> Result<EventType, AppError> {
    // EventType serde renames are the bare wire strings (`"PromptSubmitted"`, …).
    serde_json::from_value(serde_json::Value::String(s.to_string()))
        .map_err(|e| AppError::internal(format!("unknown event type {s:?}: {e}")))
}

/// Parse a stored timestamp string.
///
/// Tries the common SQLite datetime form first, then RFC3339 — matching Go
/// `scanEvents`. An unparseable timestamp is a hard error so we never invent
/// `now()` and corrupt replay order.
fn parse_timestamp(s: &str) -> Result<DateTime<Utc>, AppError> {
    // "2006-01-02 15:04:05" (Go first parse path).
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    // Fractional seconds sometimes written by SQLite drivers.
    if let Ok(naive) = NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S%.f") {
        return Ok(DateTime::<Utc>::from_naive_utc_and_offset(naive, Utc));
    }
    // RFC3339 / RFC3339Nano (Go second parse path + our write format).
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Ok(dt.with_timezone(&Utc));
    }
    Err(AppError::internal(format!("parse timestamp {s:?}")))
}

/// Run a DELETE prune query, report rows deleted, and reclaim free pages.
fn prune_with_query(
    conn: &Connection,
    label: &str,
    query: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<i64, AppError> {
    let deleted = conn
        .execute(query, params)
        .map_err(|e| AppError::internal(format!("events: {label}: {e}")))? as i64;

    if deleted > 0 {
        // PRAGMA incremental_vacuum reclaims freed pages when auto_vacuum is
        // INCREMENTAL; harmless no-op otherwise. Vacuum failure after a successful
        // delete is reported (matches Go: return deleted with wrapped vacuum err).
        if let Err(e) = conn.execute_batch("PRAGMA incremental_vacuum") {
            return Err(AppError::internal(format!(
                "events: {label} succeeded but vacuum failed: {e}"
            )));
        }
    }
    Ok(deleted)
}
