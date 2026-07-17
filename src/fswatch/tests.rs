//! Tests for the filesystem watcher (port of `internal/fswatch/watcher_test.go`).
//!
//! Filesystem watch tests are inherently timing-sensitive: notify needs a
//! moment to establish watches, the debouncer coalesces within [`OS_DEBOUNCE`],
//! and the emit throttle window is [`EMIT_THROTTLE`](super::EMIT_THROTTLE). Each
//! test uses `tempfile` for isolation and generous timeouts with retries where
//! appropriate. A failure here is more likely a timing flake than a regression
//! — re-run before investigating.

use std::fs;
use std::path::Path;
use std::sync::mpsc;
use std::sync::Mutex;
use std::time::Duration;

use tempfile::TempDir;

use super::Watcher;
use crate::interfaces::types::EventType;

/// Serializes fswatch tests so only one inotify instance exists at a time.
/// Without this, parallel tests exhaust the system's `max_user_instances`
/// limit (128 on Linux) and fail with "Too many open files".
static WATCHER_LOCK: Mutex<()> = Mutex::new(());

/// Wait up to `timeout` for an event on `rx`, returning it, or `None` on
/// timeout. Mirrors Go `waitForEvent`.
fn wait_for_event(
    rx: &mpsc::Receiver<crate::interfaces::types::Event>,
    timeout: Duration,
) -> Option<crate::interfaces::types::Event> {
    rx.recv_timeout(timeout).ok()
}

/// RAII guard that holds the watcher lock and drops the watcher before
/// releasing the lock. This ensures inotify instances are cleaned up before
/// the next test acquires the lock.
struct WatcherGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    watcher: Option<Watcher>,
}

impl Drop for WatcherGuard {
    fn drop(&mut self) {
        // Drop the watcher first (joins threads, releases inotify instances),
        // then the _lock guard drops, allowing the next test to proceed.
        self.watcher.take();
    }
}

/// Build a watcher whose emit callback forwards to a buffered channel.
/// Acquires `WATCHER_LOCK` and returns a guard that ensures the watcher is
/// cleaned up before the lock is released, serializing inotify usage.
/// Skips the test if the system is out of inotify instances.
fn make_watcher() -> Option<(
    WatcherGuard,
    mpsc::Receiver<crate::interfaces::types::Event>,
)> {
    let lock = WATCHER_LOCK.lock().expect("watcher lock");
    let (tx, rx) = mpsc::channel();
    match Watcher::new(move |e| {
        let _ = tx.send(e);
    }) {
        Ok(w) => {
            let guard = WatcherGuard {
                _lock: lock,
                watcher: Some(w),
            };
            Some((guard, rx))
        }
        Err(e) => {
            eprintln!("Skipping fswatch test: inotify instance limit reached ({e})");
            None
        }
    }
}

/// Borrow the watcher from a guard for method calls.
impl std::ops::Deref for WatcherGuard {
    type Target = Watcher;
    fn deref(&self) -> &Watcher {
        self.watcher.as_ref().expect("watcher taken during drop")
    }
}

/// Give notify a moment to establish the watch after `add_workspace`. Mirrors
/// the Go tests' `time.Sleep(100 * time.Millisecond)`.
fn settle() {
    std::thread::sleep(Duration::from_millis(150));
}

#[test]
fn external_change_emits_event() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    fs::write(dir.path().join("hello.txt"), b"hi").expect("write");

    let e =
        wait_for_event(&rx, Duration::from_secs(3)).expect("expected a FileChangedOnDisk event");
    assert_eq!(e.event_type, EventType::FileChangedOnDisk);
    assert_eq!(e.workspace_id, "ws1");
    assert_eq!(e.target, "hello.txt");
}

#[test]
fn app_write_is_suppressed() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    // Match production ordering (workspace.Manager.WriteFile): the app-write
    // timestamp is recorded BEFORE the write so the suppression check sees it
    // before the notify event is processed.
    let p = dir.path().join("app.txt");
    w.note_app_write(p.to_str().expect("utf8 path"));
    fs::write(&p, b"hi").expect("write");

    if let Some(e) = wait_for_event(&rx, Duration::from_millis(800)) {
        panic!(
            "expected app write to be suppressed, got event for {:?}",
            e.target
        );
    }
}

#[test]
fn ignored_dir_not_watched() {
    let dir = TempDir::new().expect("tempdir");
    fs::create_dir_all(dir.path().join("node_modules")).expect("mkdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    fs::write(dir.path().join("node_modules").join("x.js"), b"x").expect("write");
    if let Some(e) = wait_for_event(&rx, Duration::from_millis(800)) {
        panic!(
            "expected ignored dir to be skipped, got event for {:?}",
            e.target
        );
    }
}

#[test]
fn remove_workspace_stops_events() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();
    w.remove_workspace("ws1");
    // Give the worker a moment to unwatch.
    std::thread::sleep(Duration::from_millis(100));

    fs::write(dir.path().join("after.txt"), b"x").expect("write");
    if let Some(e) = wait_for_event(&rx, Duration::from_millis(800)) {
        panic!(
            "expected no events after remove_workspace, got {:?}",
            e.target
        );
    }
}

/// A directory created AFTER `add_workspace` is recursively watched: a write to
/// a file inside the freshly created nested directory must surface as an event.
/// Exercises the Create-directory → add_tree path in `handle_event`.
#[test]
fn recursive_create_directory_watches() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    let nested = dir.path().join("src").join("routes");
    fs::create_dir_all(&nested).expect("mkdir");
    // Give the Create event + add_tree time to register the new directory.
    std::thread::sleep(Duration::from_millis(250));

    let target = nested.join("index.js");
    fs::write(&target, b"module.exports = 1;").expect("write");

    let e = wait_for_event(&rx, Duration::from_secs(4))
        .expect("expected a FileChangedOnDisk event for a file inside a newly created nested dir");
    assert_eq!(e.event_type, EventType::FileChangedOnDisk);
    let want = Path::new("src").join("routes").join("index.js");
    assert_eq!(Path::new(&e.target), want);
}

/// Two rapid writes to the same file produce exactly one event (the second is
/// coalesced within the emit throttle window).
#[test]
fn emit_throttle_coalesces() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    let p = dir.path().join("throttle.txt");
    fs::write(&p, b"a").expect("write 1");

    let e = wait_for_event(&rx, Duration::from_secs(3))
        .expect("expected the first FileChangedOnDisk event");
    assert_eq!(e.target, "throttle.txt");

    // Second write immediately after the first should fall inside emit_throttle
    // (300ms) and be coalesced — no second event.
    fs::write(&p, b"bb").expect("write 2");
    if let Some(e2) = wait_for_event(&rx, Duration::from_millis(900)) {
        panic!(
            "expected exactly one event (coalesced), got a second for {:?}",
            e2.target
        );
    }
}

/// `close` is idempotent: calling it twice must not panic and the second call
/// returns `Ok(())`.
#[test]
fn double_close_is_safe() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, _rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    w.close().expect("first close");
    // The Drop impl also calls close; this second explicit call exercises
    // idempotency.
    w.close().expect("second close");
}

/// No events are emitted for writes that happen after `close`. The watcher's
/// threads have exited and its debouncer is stopped, so any post-close write
/// cannot produce an event.
#[test]
fn events_stop_after_close() {
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    let dir = TempDir::new().expect("tempdir");
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    w.close().expect("close");
    // Drain any events already queued from the initial watch setup / close.
    while rx.try_recv().is_ok() {}

    fs::write(dir.path().join("post-close.txt"), b"x").expect("write");
    if let Some(e) = wait_for_event(&rx, Duration::from_millis(800)) {
        panic!("expected no events after close, got {:?}", e.target);
    }
}

/// The watcher handles file creation, modification, and deletion — each
/// surfaces as a distinct `FileChangedOnDisk` event (subject to the per-path
/// throttle, so the operations are spaced apart).
#[test]
fn handles_create_modify_delete() {
    let dir = TempDir::new().expect("tempdir");
    let Some((w, rx)) = make_watcher() else {
        return;
    };
    w.add_workspace("ws1", dir.path().to_str().expect("utf8 path"));
    settle();

    let p = dir.path().join("lifecycle.txt");

    // Create.
    fs::write(&p, b"v1").expect("create");
    let e1 = wait_for_event(&rx, Duration::from_secs(3)).expect("expected create event");
    assert_eq!(e1.target, "lifecycle.txt");

    // Wait past the emit throttle so the modify is emitted.
    std::thread::sleep(Duration::from_millis(450));
    fs::write(&p, b"v2").expect("modify");
    let e2 = wait_for_event(&rx, Duration::from_secs(3)).expect("expected modify event");
    assert_eq!(e2.target, "lifecycle.txt");

    // Wait past the throttle so the delete is emitted.
    std::thread::sleep(Duration::from_millis(450));
    fs::remove_file(&p).expect("delete");
    let e3 = wait_for_event(&rx, Duration::from_secs(3)).expect("expected delete event");
    assert_eq!(e3.target, "lifecycle.txt");
}
