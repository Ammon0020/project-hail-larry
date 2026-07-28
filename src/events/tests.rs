//! Event store + bus tests (port of `internal/events/events_test.go` plus
//! concurrent append, PRAGMA, persist-before-publish, and reconnect handoff).

use std::sync::Arc;
use std::time::Duration;

use tempfile::TempDir;

use super::publisher::EventBus;
use super::store::Store;
use crate::interfaces::types::{go_zero_time, Attachment, Event, EventType};
use crate::interfaces::{AppError, EventPublisher, EventStore};

/// Temp `SQLite` store cleaned up when the `TempDir` drops.
#[allow(clippy::unused_async)]
// kept async for caller await: many tests await this helper for symmetry with async store APIs.
async fn new_test_store() -> (Store, TempDir) {
    let dir = TempDir::new().expect("tempdir");
    let path = dir.path().join("test_events.db");
    let store = Store::open(&path).expect("open store");
    (store, dir)
}

async fn new_test_bus() -> (EventBus, TempDir) {
    let (store, dir) = new_test_store().await;
    (EventBus::new(store), dir)
}

fn minimal_event(session_id: &str, event_type: EventType, content: &str) -> Event {
    let mut e = Event::new(0, event_type, session_id, go_zero_time());
    e.content = content.into();
    e
}

// ---------------------------------------------------------------------------
// Ported Go tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn append_and_query() {
    let (store, _dir) = new_test_store().await;

    let mut e1 = minimal_event("session-1", EventType::PromptSubmitted, "Hello, agent!");
    e1.role = "user".into();
    let e1 = store.append(e1).await.expect("append e1");
    assert_ne!(e1.id, 0, "expected non-zero event ID");

    let mut e2 = minimal_event("session-1", EventType::ResponseStarted, "Hello, human!");
    e2.role = "agent".into();
    let e2 = store.append(e2).await.expect("append e2");
    assert!(e2.id > e1.id, "expected e2.id > e1.id");

    let events = store.query("session-1", 0, 100).await.expect("query");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].event_type, EventType::PromptSubmitted);
    assert_eq!(events[0].content, "Hello, agent!");
    assert_eq!(events[0].role, "user");
    assert_eq!(events[1].event_type, EventType::ResponseStarted);
    assert_eq!(events[1].content, "Hello, human!");
}

#[tokio::test]
async fn query_with_cursor() {
    let (store, _dir) = new_test_store().await;

    let e1 = store
        .append(minimal_event("s1", EventType::PromptSubmitted, "first"))
        .await
        .expect("e1");
    let e2 = store
        .append(minimal_event("s1", EventType::ResponseStarted, "second"))
        .await
        .expect("e2");
    store
        .append(minimal_event("s1", EventType::StreamUpdate, "third"))
        .await
        .expect("e3");

    let events = store.query("s1", e1.id, 100).await.expect("query");
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, e2.id);
}

#[tokio::test]
async fn query_before_returns_the_tail_in_chronological_order() {
    let (store, _dir) = new_test_store().await;
    let mut ids = Vec::new();
    for content in ["first", "second", "third"] {
        ids.push(
            store
                .append(minimal_event("s1", EventType::StreamUpdate, content))
                .await
                .expect("append event")
                .id,
        );
    }

    let tail = store.query_before("s1", 0, 2).await.expect("query tail");
    assert_eq!(
        tail.iter().map(|event| event.id).collect::<Vec<_>>(),
        ids[1..]
    );

    let older = store
        .query_before("s1", tail[0].id, 2)
        .await
        .expect("query older page");
    assert_eq!(
        older.iter().map(|event| event.id).collect::<Vec<_>>(),
        ids[..1]
    );
}

#[tokio::test]
async fn query_different_sessions() {
    let (store, _dir) = new_test_store().await;

    store
        .append(minimal_event("s1", EventType::PromptSubmitted, "session 1"))
        .await
        .unwrap();
    store
        .append(minimal_event("s2", EventType::PromptSubmitted, "session 2"))
        .await
        .unwrap();
    store
        .append(minimal_event(
            "s1",
            EventType::PromptSubmitted,
            "session 1 again",
        ))
        .await
        .unwrap();

    let s1 = store.query("s1", 0, 100).await.unwrap();
    assert_eq!(s1.len(), 2);
    let s2 = store.query("s2", 0, 100).await.unwrap();
    assert_eq!(s2.len(), 1);
}

#[tokio::test]
async fn query_all() {
    let (store, _dir) = new_test_store().await;

    for (sid, c) in [("s1", "a"), ("s2", "b"), ("s3", "c")] {
        store
            .append(minimal_event(sid, EventType::PromptSubmitted, c))
            .await
            .unwrap();
    }

    let events = store.query_all(0, 100).await.unwrap();
    assert_eq!(events.len(), 3);
}

#[tokio::test]
async fn query_empty() {
    let (store, _dir) = new_test_store().await;
    let events = store.query("nonexistent", 0, 100).await.unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn append_tool_event() {
    let (store, _dir) = new_test_store().await;

    let mut e = Event::new(0, EventType::ToolCompleted, "s1", go_zero_time());
    e.tool = "edit_file".into();
    e.target = "server.js".into();
    e.summary = "Added error handler at line 17-21".into();
    store.append(e).await.unwrap();

    let events = store.query("s1", 0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].tool, "edit_file");
    assert_eq!(events[0].target, "server.js");
    assert_eq!(events[0].summary, "Added error handler at line 17-21");
}

#[tokio::test]
async fn append_attachments_event() {
    let (store, _dir) = new_test_store().await;

    let mut e = Event::new(0, EventType::PromptSubmitted, "s1", go_zero_time());
    e.role = "user".into();
    e.content = "see attached".into();
    e.attachments = vec![Attachment {
        id: "abc123def456".into(),
        name: "test.png".into(),
        mime_type: "image/png".into(),
        uri: String::new(),
        path: "/some/path".into(),
    }];
    store.append(e).await.unwrap();

    let events = store.query("s1", 0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].attachments.len(), 1);
    let a = &events[0].attachments[0];
    assert_eq!(a.id, "abc123def456");
    assert_eq!(a.name, "test.png");
    assert_eq!(a.mime_type, "image/png");
    assert_eq!(a.path, "/some/path");
}

#[tokio::test]
async fn append_permission_event() {
    let (store, _dir) = new_test_store().await;

    let mut e = Event::new(0, EventType::PermissionRequested, "s1", go_zero_time());
    e.tool = "shell".into();
    e.command = "npm test".into();
    e.options = vec![
        "allow_once".into(),
        "allow_session".into(),
        "allow_always".into(),
        "deny".into(),
    ];
    store.append(e).await.unwrap();

    let events = store.query("s1", 0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].command, "npm test");
    assert_eq!(events[0].options.len(), 4);
}

#[tokio::test]
async fn prune_by_row_count() {
    let (store, _dir) = new_test_store().await;

    for _ in 0..5 {
        store
            .append(minimal_event("s1", EventType::PromptSubmitted, "e"))
            .await
            .unwrap();
    }
    assert_eq!(store.count().await.unwrap(), 5);

    let deleted = store.prune(2).await.unwrap();
    assert_eq!(deleted, 3);
    assert_eq!(store.count().await.unwrap(), 2);

    let events = store.query_all(0, 100).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].id, 4);
    assert_eq!(events[1].id, 5);
}

#[tokio::test]
async fn prune_no_op_when_under_limit() {
    let (store, _dir) = new_test_store().await;
    store
        .append(minimal_event("s1", EventType::PromptSubmitted, ""))
        .await
        .unwrap();
    store
        .append(minimal_event("s1", EventType::PromptSubmitted, ""))
        .await
        .unwrap();

    let deleted = store.prune(10).await.unwrap();
    assert_eq!(deleted, 0);
    assert_eq!(store.count().await.unwrap(), 2);
}

#[tokio::test]
async fn prune_rejects_non_positive() {
    let (store, _dir) = new_test_store().await;
    store
        .append(minimal_event("s1", EventType::PromptSubmitted, ""))
        .await
        .unwrap();

    assert!(store.prune(0).await.is_err());
    assert!(store.prune(-1).await.is_err());
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn prune_older_than() {
    let (store, _dir) = new_test_store().await;

    let mut old = minimal_event("s1", EventType::PromptSubmitted, "old");
    old.timestamp = chrono::Utc::now() - chrono::Duration::hours(2);
    let mut recent = minimal_event("s1", EventType::PromptSubmitted, "recent");
    recent.timestamp = chrono::Utc::now();

    store.append(old).await.unwrap();
    store.append(recent).await.unwrap();

    let deleted = store
        .prune_older_than(Duration::from_secs(3600))
        .await
        .unwrap();
    assert_eq!(deleted, 1);
    assert_eq!(store.count().await.unwrap(), 1);

    let events = store.query_all(0, 100).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].content, "recent");
}

#[tokio::test]
async fn prune_older_than_rejects_non_positive() {
    let (store, _dir) = new_test_store().await;
    store
        .append(minimal_event("s1", EventType::PromptSubmitted, ""))
        .await
        .unwrap();

    assert!(store.prune_older_than(Duration::ZERO).await.is_err());
    assert_eq!(store.count().await.unwrap(), 1);
}

#[tokio::test]
async fn start_prune_ticker() {
    let (store, _dir) = new_test_store().await;

    for _ in 0..5 {
        store
            .append(minimal_event("s1", EventType::PromptSubmitted, ""))
            .await
            .unwrap();
    }

    let handle = store.start_prune_ticker(Duration::from_millis(10), 2);

    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    loop {
        if store.count().await.unwrap() <= 2 {
            break;
        }
        assert!(
            tokio::time::Instant::now() <= deadline,
            "expected prune ticker to reduce to 2 events"
        );
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert_eq!(store.count().await.unwrap(), 2);

    handle.abort();
    // Abort is idempotent from the caller's perspective.
    handle.abort();
}

#[tokio::test]
async fn start_prune_ticker_defaults() {
    let (store, _dir) = new_test_store().await;
    let handle = store.start_prune_ticker(Duration::ZERO, 0);
    handle.abort();
}

// ---------------------------------------------------------------------------
// New Rust tests (PRAGMA, concurrency, publisher handoff)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn wal_mode_and_busy_timeout() {
    let (store, _dir) = new_test_store().await;

    let journal = store.pragma("journal_mode").await.unwrap();
    assert_eq!(journal.to_lowercase(), "wal");

    // busy_timeout may be returned as an integer or string depending on the
    // SQLite build; open() sets 5000 ms.
    let busy = store.pragma("busy_timeout").await.unwrap();
    assert!(
        busy == "5000" || busy.parse::<i64>().ok() == Some(5000),
        "expected busy_timeout 5000, got {busy}"
    );
}

#[tokio::test]
async fn default_query_limit_when_non_positive() {
    let (store, _dir) = new_test_store().await;
    // limit <= 0 falls back to 1000 (Go behavior) — smoke-check with empty result.
    let events = store.query_all(0, 0).await.unwrap();
    assert!(events.is_empty());
    let events = store.query("s", 0, -1).await.unwrap();
    assert!(events.is_empty());
}

#[tokio::test]
async fn concurrent_appends_monotone_ids() {
    let (store, _dir) = new_test_store().await;
    let store = Arc::new(store);

    let n = 50;
    let mut handles = Vec::with_capacity(n);
    for i in 0..n {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            s.append(minimal_event(
                "s1",
                EventType::StreamUpdate,
                &format!("c{i}"),
            ))
            .await
        }));
    }

    let mut ids = Vec::with_capacity(n);
    for h in handles {
        let e = h.await.expect("join").expect("append");
        ids.push(e.id);
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), n, "IDs must be unique");
    assert_eq!(ids[0], 1);
    // `n` is the small constant 50, so the wrap cast is impossible.
    #[allow(clippy::cast_possible_wrap)]
    let last_id = n as i64;
    assert_eq!(ids[n - 1], last_id);

    let events = store.query_all(0, 1000).await.unwrap();
    assert_eq!(events.len(), n);
    // Ordering by id ASC is guaranteed.
    for w in events.windows(2) {
        assert!(w[0].id < w[1].id);
    }
}

#[tokio::test]
async fn no_lock_contention_under_concurrent_appends() {
    // Single connection + busy_timeout: concurrent appends must all succeed
    // without SQLITE_BUSY / "database is locked" errors.
    let (store, _dir) = new_test_store().await;
    let store = Arc::new(store);

    let mut handles = Vec::new();
    for i in 0..32 {
        let s = Arc::clone(&store);
        handles.push(tokio::spawn(async move {
            for j in 0..5 {
                s.append(minimal_event(
                    &format!("s{}", i % 4),
                    EventType::PromptSubmitted,
                    &format!("{i}-{j}"),
                ))
                .await
                .map_err(|e: AppError| e.to_string())?;
            }
            Ok::<(), String>(())
        }));
    }

    for h in handles {
        h.await.expect("join").expect("append batch");
    }
    assert_eq!(store.count().await.unwrap(), 32 * 5);
}

#[tokio::test]
async fn persist_before_publish_ordering() {
    let (bus, _dir) = new_test_bus().await;

    // Subscribe first so we observe the live delivery.
    let mut sub = bus.subscribe(0).await.unwrap();

    let stored = bus
        .append_and_publish(minimal_event("s1", EventType::PromptSubmitted, "hello"))
        .await
        .unwrap();
    assert_ne!(stored.id, 0);

    // Durable row must exist before/when subscriber receives it.
    let durable = bus.store().query_all(0, 10).await.unwrap();
    assert_eq!(durable.len(), 1);
    assert_eq!(durable[0].id, stored.id);

    let received = tokio::time::timeout(Duration::from_secs(1), sub.recv())
        .await
        .expect("timeout waiting for event")
        .expect("subscription closed");
    assert_eq!(received.id, stored.id);
    assert_eq!(received.content, "hello");
}

#[tokio::test]
async fn publish_requires_durable_id() {
    let (bus, _dir) = new_test_bus().await;
    let e = minimal_event("s1", EventType::PromptSubmitted, "x");
    let err = bus.publish(&e).await.expect_err("id=0 must fail");
    assert!(err.to_string().contains("durable event id"));
}

#[tokio::test]
async fn reconnect_replay_handoff_dedupes_by_id() {
    let (bus, _dir) = new_test_bus().await;

    // Seed three durable events without a live subscriber.
    let e1 = bus
        .append_and_publish(minimal_event("s1", EventType::PromptSubmitted, "1"))
        .await
        .unwrap();
    let e2 = bus
        .append_and_publish(minimal_event("s1", EventType::ResponseStarted, "2"))
        .await
        .unwrap();
    let e3 = bus
        .append_and_publish(minimal_event("s1", EventType::StreamUpdate, "3"))
        .await
        .unwrap();

    // Client reconnects with cursor after e1 → should replay e2, e3 then live.
    let mut sub = bus.subscribe(e1.id).await.unwrap();

    let r1 = sub.recv().await.expect("replay e2");
    let r2 = sub.recv().await.expect("replay e3");
    assert_eq!(r1.id, e2.id);
    assert_eq!(r2.id, e3.id);
    assert_eq!(sub.last_seen_id(), e3.id);

    // Live event after reconnect.
    let e4 = bus
        .append_and_publish(minimal_event("s1", EventType::StreamUpdate, "4"))
        .await
        .unwrap();
    let r3 = tokio::time::timeout(Duration::from_secs(1), sub.recv())
        .await
        .expect("timeout")
        .expect("live e4");
    assert_eq!(r3.id, e4.id);
    assert_eq!(r3.content, "4");
}

#[tokio::test]
async fn append_then_publish_via_traits() {
    // Trait-level path: EventStore::append then EventPublisher::publish.
    let (bus, _dir) = new_test_bus().await;
    let mut sub = bus.subscribe(0).await.unwrap();

    let stored = EventStore::append(
        &bus,
        minimal_event("s1", EventType::PromptSubmitted, "via-trait"),
    )
    .await
    .unwrap();
    // Not published yet — subscription should not see it as live (only via
    // future re-subscribe replay). We published after append:
    EventPublisher::publish(&bus, &stored).await.unwrap();

    let got = tokio::time::timeout(Duration::from_secs(1), sub.recv())
        .await
        .expect("timeout")
        .expect("event");
    // Subscriber registered before append, so the event arrives either via
    // live publish (id match) — and must have the durable id.
    assert_eq!(got.id, stored.id);
    assert_eq!(got.content, "via-trait");
}

#[tokio::test]
async fn timestamp_set_when_zero() {
    let (store, _dir) = new_test_store().await;
    let e = store
        .append(minimal_event("s1", EventType::PromptSubmitted, "t"))
        .await
        .unwrap();
    assert_ne!(e.timestamp, go_zero_time());
    // Round-trip preserves a real timestamp.
    let events = store.query("s1", 0, 1).await.unwrap();
    assert_eq!(events[0].timestamp, e.timestamp);
}

#[tokio::test]
async fn parses_go_time_string_timestamps_from_legacy_db() {
    use rusqlite::Connection;

    let dir = TempDir::new().expect("tempdir");
    let db_path = dir.path().join("legacy_events.db");
    // Create schema via Store, then release so we can insert raw Go timestamps.
    {
        let _store = Store::open(&db_path).expect("open store");
    }

    let conn = Connection::open(&db_path).unwrap();
    // Legacy Go database/sql encoding of time.Time (see user DB row id=1).
    conn.execute(
        "INSERT INTO events (type, session_id, timestamp, payload) VALUES (?1, ?2, ?3, ?4)",
        rusqlite::params![
            "PromptSubmitted",
            "legacy-sess",
            "2026-06-30 02:18:34.296832823 +0000 UTC",
            r#"{"role":"user","content":"hi"}"#,
        ],
    )
    .unwrap();
    drop(conn);

    let store = Store::open(&db_path).unwrap();
    let events = store.query("legacy-sess", 0, 10).await.unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].content, "hi");
    assert_eq!(
        events[0].timestamp.to_rfc3339(),
        "2026-06-30T02:18:34.296832823+00:00"
    );
}
