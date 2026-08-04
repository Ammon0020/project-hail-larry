//! On-disk change detection (Go `internal/fswatch`).
//!
//! Watches registered workspace roots for file changes that originate OUTSIDE
//! the app (e.g. a file edited in another editor) and emits
//! [`EventType::FileChangedOnDisk`] for each. The app's own writes are
//! suppressed (agent writes already emit `FileWritten` and user saves are
//! already reflected in the editor), so this fires only for external changes.
//!
//! # Architecture
//!
//! Three std threads cooperate, mirroring the Go goroutine layout:
//!
//! - **Debouncer thread** (owned by [`notify_debouncer_full`]): coalesces the
//!   OS-level event burst a single editor save produces into one
//!   [`DebouncedEvent`] per path, then invokes our callback. The callback
//!   applies ignore/suppression/throttle rules and emits to a bounded channel.
//! - **Worker thread**: owns the [`Debouncer`] and services watch/unwatch
//!   commands (`Command`) from the public API and `WatchNewDir` requests
//!   from the callback (recursive coverage for newly created directories).
//! - **Emit thread**: drains the bounded emit channel and invokes the user
//!   callback, so a slow callback (WebSocket broadcast, event store append)
//!   never blocks the watcher.
//!
//! Shared mutable state (workspace roots, app-write suppression set, per-path
//! emit throttle) lives behind a single [`Mutex`] in `SharedState`. The
//! suppression and throttle maps are bounded [`LruCache`]s so they cannot grow
//! unbounded between cleanup passes.
//!
//! Blueprint reference: Sec 14 (File System Access — external change detection).

use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use lru::LruCache;
use notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{
    new_debouncer, DebounceEventResult, DebouncedEvent, Debouncer, RecommendedCache,
};
use tracing::{debug, warn};

use crate::interfaces::types::{go_zero_time, Event, EventType};
use crate::interfaces::AppError;

/// Directory names never watched. Mirrors `internal/search` and the file-tree
/// behavior so watch events correspond to what the UI actually shows.
const IGNORE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "vendor",
    "dist",
    "build",
    ".next",
    "target",
    ".cache",
    "coverage",
    "out",
];

/// How long after an app write a matching filesystem event is suppressed. The
/// app's own writes emit `FileWritten` elsewhere; this avoids a redundant
/// `FileChangedOnDisk` for the same change. Matches Go `appWriteSuppression`.
const APP_WRITE_SUPPRESSION: Duration = Duration::from_secs(2);

/// Per-path emit throttle: two events for the same path within this window
/// coalesce into one. Editors often write a file several times during a single
/// save. Matches Go `emitThrottle`.
const EMIT_THROTTLE: Duration = Duration::from_millis(300);

/// Interval between suppression/throttle bookkeeping cleanup passes. Matches
/// Go `cleanupInterval`. Cleanup runs opportunistically in the event callback.
const CLEANUP_INTERVAL: Duration = Duration::from_secs(30);

/// OS-level debounce window. Coalesces the burst of low-level notify events a
/// single editor save produces (create + write + rename + …) into one debounced
/// event. Kept short so newly created directories are watched before the next
/// write arrives. The per-path [`EMIT_THROTTLE`] handles coarser coalescing.
const OS_DEBOUNCE: Duration = Duration::from_millis(60);

/// Hard cap on the app-write suppression and emit-throttle maps. The
/// opportunistic cleanup evicts time-expired entries every 30s; this cap
/// prevents unbounded growth under a burst of distinct paths between cleanups.
const STATE_CAP: usize = 4096;

/// Capacity of the buffered emit channel. A full channel drops the event
/// rather than blocking the watcher thread (matches Go's `default:` drop).
const EMIT_CHANNEL_CAPACITY: usize = 64;

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Filesystem watcher for external on-disk changes.
///
/// Cheap to share: all mutable state is behind internal locks. Call
/// [`Watcher::close`] to stop the background threads (idempotent); [`Drop`]
/// also performs best-effort cleanup.
pub struct Watcher {
    /// Workspace roots, app-write suppression set, per-path emit throttle.
    state: Arc<Mutex<SharedState>>,
    /// Sends watch/unwatch commands to the worker thread.
    command_tx: Mutex<mpsc::Sender<Command>>,
    /// Worker thread handle (taken out on close).
    worker: Mutex<Option<JoinHandle<()>>>,
    /// Emit thread handle (taken out on close).
    emit: Mutex<Option<JoinHandle<()>>>,
}

impl Watcher {
    /// Create a watcher and start its background threads.
    ///
    /// `emit` is invoked for each external file change. It may be slow
    /// (WebSocket broadcast, event store append): it runs on a dedicated emit
    /// thread draining a bounded channel, so it never blocks the watcher.
    ///
    /// # Errors
    /// Returns [`AppError::internal`] only if the underlying notify watcher
    /// cannot be created or a background thread fails to spawn.
    pub fn new<F>(emit: F) -> Result<Self, AppError>
    where
        F: Fn(Event) + Send + Sync + 'static,
    {
        let state = Arc::new(Mutex::new(SharedState::new()));
        let (command_tx, command_rx) = mpsc::channel::<Command>();
        let (emit_tx, emit_rx) = mpsc::sync_channel::<Event>(EMIT_CHANNEL_CAPACITY);

        // The debouncer callback runs on the debouncer's internal thread. It
        // captures clones of the shared state, the command sender (for
        // WatchNewDir requests), and the emit sender.
        let cb_state = Arc::clone(&state);
        let cb_cmd = command_tx.clone();
        let cb_emit = emit_tx.clone();
        let debouncer = new_debouncer(OS_DEBOUNCE, None, move |res: DebounceEventResult| {
            handle_debounce_result(res, &cb_state, &cb_cmd, &cb_emit);
        })
        .map_err(|e| AppError::internal(format!("create file watcher: {e}")))?;

        // The worker owns the debouncer (for &mut watch/unwatch) and the
        // command receiver. It maintains the watched-path set.
        let worker = thread::Builder::new()
            .name("fswatch-worker".into())
            .spawn(move || worker_loop(debouncer, command_rx))
            .map_err(|e| AppError::internal(format!("spawn fswatch worker: {e}")))?;

        // The emit thread drains the bounded channel and invokes the user
        // callback. It exits when all emit senders drop (i.e. once the
        // debouncer thread stops and the callback is dropped).
        let emit_callback = Arc::new(emit);
        let emit_handle = thread::Builder::new()
            .name("fswatch-emit".into())
            .spawn(move || emit_loop(emit_rx, emit_callback))
            .map_err(|e| AppError::internal(format!("spawn fswatch emit: {e}")))?;

        Ok(Self {
            state,
            command_tx: Mutex::new(command_tx),
            worker: Mutex::new(Some(worker)),
            emit: Mutex::new(Some(emit_handle)),
        })
    }

    /// Begin watching a workspace root recursively.
    ///
    /// The tree is walked once and every non-ignored, non-hidden directory is
    /// added (notify on Linux/inotify is non-recursive, so each directory is
    /// watched individually; new subdirectories are watched on the fly as they
    /// are created). Per-path errors are logged and skipped so one bad
    /// directory doesn't abort the rest.
    ///
    /// If `id` is already registered with a different path, the old watches are
    /// removed first so stale watches are not leaked. No-op if the watcher is
    /// closed.
    pub fn add_workspace(&self, id: &str, abs_path: &str) {
        let path = PathBuf::from(abs_path);
        let old_path = {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if s.closed {
                return;
            }
            // Re-add with a different path: unwatch the old root first.
            let old = s
                .roots
                .get(id)
                .filter(|r| r.as_path() != path.as_path())
                .cloned();
            s.roots.insert(id.to_string(), path.clone());
            old
        };
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if let Some(old) = old_path {
            let _ = tx.send(Command::RemoveWorkspace(old));
        }
        let _ = tx.send(Command::AddWorkspace(path));
    }

    /// Stop watching a workspace root and all its subdirectories. No-op if the
    /// watcher is closed or `id` is not registered.
    pub fn remove_workspace(&self, id: &str) {
        let path = {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if s.closed {
                return;
            }
            s.roots.remove(id)
        };
        if let Some(path) = path {
            let _ = self
                .command_tx
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .send(Command::RemoveWorkspace(path));
        }
    }

    /// Record that the app itself just wrote `abs_path` so the imminent
    /// filesystem event for it is suppressed (not surfaced as an external
    /// change). No-op if the watcher is closed, so shutdown-time hook calls
    /// don't leak entries into the suppression set.
    pub fn note_app_write(&self, abs_path: &str) {
        let path = PathBuf::from(abs_path);
        let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        if s.closed {
            return;
        }
        s.app_writes.put(path, Instant::now());
    }

    /// Stop the watcher's background threads and release resources.
    ///
    /// Idempotent: calling it more than once returns `Ok(())` without
    /// re-joining. Thread panics during join are logged and swallowed (cleanup
    /// is best-effort).
    ///
    /// # Errors
    ///
    /// Returns an error if a background thread fails to join within the timeout.
    pub fn close(&self) -> Result<(), AppError> {
        {
            let mut s = self.state.lock().unwrap_or_else(PoisonError::into_inner);
            if s.closed {
                return Ok(());
            }
            s.closed = true;
        }
        // Signal the worker to stop. Ignore send errors (worker may have exited).
        let _ = self
            .command_tx
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .send(Command::Close);
        // Join the worker: it stops the debouncer (joining the debouncer
        // thread, which drops the callback and the emit sender), which in turn
        // stops the emit thread.
        if let Some(handle) = self
            .worker
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            if let Err(e) = handle.join() {
                warn!("fswatch worker thread panicked: {e:?}");
            }
        }
        if let Some(handle) = self
            .emit
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            if let Err(e) = handle.join() {
                warn!("fswatch emit thread panicked: {e:?}");
            }
        }
        Ok(())
    }
}

impl Drop for Watcher {
    fn drop(&mut self) {
        // Best-effort cleanup if close() wasn't called explicitly.
        let _ = self.close();
    }
}

// ---------------------------------------------------------------------------
// Shared state
// ---------------------------------------------------------------------------

/// Mutex-protected watcher bookkeeping shared between the public API, the
/// debouncer callback, and (transitively) the emit thread.
struct SharedState {
    /// workspaceID -> absolute root path. Updated synchronously by the public
    /// API so the callback can resolve the owning workspace immediately.
    roots: HashMap<String, PathBuf>,
    /// absPath -> time the app wrote it. Bounded; entries expire by TTL during
    /// opportunistic cleanup.
    app_writes: LruCache<PathBuf, Instant>,
    /// absPath -> last emit time (per-path throttle). Bounded; entries expire
    /// during opportunistic cleanup.
    last_emit: LruCache<PathBuf, Instant>,
    /// When the last opportunistic cleanup pass ran.
    last_cleanup: Instant,
    /// True once `close()` has run; further public-API calls become no-ops.
    closed: bool,
}

impl SharedState {
    fn new() -> Self {
        let cap = NonZeroUsize::new(STATE_CAP).unwrap_or(NonZeroUsize::MIN);
        Self {
            roots: HashMap::new(),
            app_writes: LruCache::new(cap),
            last_emit: LruCache::new(cap),
            last_cleanup: Instant::now(),
            closed: false,
        }
    }

    /// Evict time-expired suppression and throttle entries. Bounded by the LRU
    /// cap regardless, but this keeps the maps small so peek stays fast and
    /// honors the "explicit cleanup" requirement.
    fn cleanup(&mut self, now: Instant) {
        let expired_aw: Vec<PathBuf> = self
            .app_writes
            .iter()
            .filter(|(_, t)| now.duration_since(**t) > APP_WRITE_SUPPRESSION)
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired_aw {
            self.app_writes.pop(&k);
        }
        let expired_le: Vec<PathBuf> = self
            .last_emit
            .iter()
            .filter(|(_, t)| now.duration_since(**t) > CLEANUP_INTERVAL)
            .map(|(k, _)| k.clone())
            .collect();
        for k in expired_le {
            self.last_emit.pop(&k);
        }
        self.last_cleanup = now;
    }
}

// ---------------------------------------------------------------------------
// Commands (public API / callback -> worker thread)
// ---------------------------------------------------------------------------

enum Command {
    AddWorkspace(PathBuf),
    RemoveWorkspace(PathBuf),
    /// A directory was just created; watch it and its tree recursively.
    WatchNewDir(PathBuf),
    Close,
}

// ---------------------------------------------------------------------------
// Worker thread: owns the Debouncer, services watch/unwatch commands
// ---------------------------------------------------------------------------

/// Worker loop: owns the debouncer and the watched-path set. Exits on
/// `Command::Close` or when all command senders drop, then stops the debouncer
/// (joining its internal thread so the callback fully stops before exit).
// Worker entry point owns the receiver for its lifetime; borrowing would force lifetime plumbing.
#[allow(clippy::needless_pass_by_value)]
fn worker_loop(
    mut debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
    command_rx: Receiver<Command>,
) {
    let mut watched: HashSet<PathBuf> = HashSet::new();
    loop {
        match command_rx.recv() {
            Ok(Command::AddWorkspace(p) | Command::WatchNewDir(p)) => {
                add_tree(&mut debouncer, &p, &mut watched);
            }
            Ok(Command::RemoveWorkspace(p)) => remove_tree(&mut debouncer, &p, &mut watched),
            Ok(Command::Close) | Err(_) => break,
        }
    }
    // Stop the debouncer (joins its internal thread so the callback — and the
    // emit sender it captured — is dropped before we exit). This in turn lets
    // the emit thread observe channel closure and exit.
    debouncer.stop();
}

/// Walk `root` and add every non-ignored, non-hidden directory (`NonRecursive`
/// per dir, matching Go's fsnotify usage). Idempotent via the `watched` set.
fn add_tree(
    debouncer: &mut Debouncer<RecommendedWatcher, RecommendedCache>,
    root: &Path,
    watched: &mut HashSet<PathBuf>,
) {
    walk_dirs(root, root, debouncer, watched);
}

/// Recursive directory walker. `root` is the workspace root (exempt from the
/// ignore/hidden check); `dir` is the current directory being visited.
fn walk_dirs(
    root: &Path,
    dir: &Path,
    debouncer: &mut Debouncer<RecommendedWatcher, RecommendedCache>,
    watched: &mut HashSet<PathBuf>,
) {
    let name = dir.file_name().and_then(OsStr::to_str).unwrap_or("");
    let is_root = dir == root;
    // Skip ignored/hidden directories (except the workspace root itself).
    if !is_root && (is_ignored_dir_name(name) || name.starts_with('.')) {
        return;
    }
    // Add this directory if not already watched.
    if !watched.contains(dir) {
        match debouncer.watch(dir, RecursiveMode::NonRecursive) {
            Ok(()) => {
                watched.insert(dir.to_path_buf());
            }
            Err(e) => {
                warn!("fswatch: add {}: {e}", dir.display());
                // Don't record in `watched` so a later retry can succeed.
            }
        }
    }
    // Recurse into subdirectories. Use `DirEntry::file_type` (which does NOT
    // follow symlinks) and skip symlinks so a symlinked subdir can't pull the
    // watcher outside the workspace root (e.g. proj/evil -> /etc).
    match fs::read_dir(dir) {
        Ok(entries) => {
            for entry in entries.flatten() {
                let path = entry.path();
                let Ok(ft) = entry.file_type() else { continue };
                if ft.is_symlink() || !ft.is_dir() {
                    continue;
                }
                walk_dirs(root, &path, debouncer, watched);
            }
        }
        Err(e) => warn!("fswatch: read_dir {}: {e}", dir.display()),
    }
}

/// Unwatch `root` and all paths under it. Iterates the watched set rather than
/// notify's watch list (notify doesn't expose a watch list in the public API).
fn remove_tree(
    debouncer: &mut Debouncer<RecommendedWatcher, RecommendedCache>,
    root: &Path,
    watched: &mut HashSet<PathBuf>,
) {
    let to_remove: Vec<PathBuf> = watched
        .iter()
        .filter(|p| path_under_root(p, root))
        .cloned()
        .collect();
    for p in to_remove {
        if let Err(e) = debouncer.unwatch(&p) {
            warn!("fswatch: unwatch {}: {e}", p.display());
        }
        watched.remove(&p);
    }
}

// ---------------------------------------------------------------------------
// Emit thread: drains the bounded channel and invokes the user callback
// ---------------------------------------------------------------------------

// Worker entry point owns the receiver and callback for its lifetime; borrowing would force lifetime plumbing.
#[allow(clippy::needless_pass_by_value)]
fn emit_loop(emit_rx: Receiver<Event>, emit_callback: Arc<dyn Fn(Event) + Send + Sync>) {
    while let Ok(event) = emit_rx.recv() {
        emit_callback(event);
    }
}

// ---------------------------------------------------------------------------
// Debouncer callback: ignore/suppress/throttle rules, then emit
// ---------------------------------------------------------------------------

/// Entry point for each batch of debounced events from the debouncer thread.
fn handle_debounce_result(
    res: DebounceEventResult,
    state: &Mutex<SharedState>,
    command_tx: &mpsc::Sender<Command>,
    emit_tx: &SyncSender<Event>,
) {
    match res {
        Ok(events) => {
            for ev in events {
                handle_event(&ev, state, command_tx, emit_tx);
            }
        }
        Err(errs) => {
            for e in errs {
                warn!("fswatch: {e}");
            }
        }
    }
}

/// Apply ignore/suppression/throttle rules to one debounced event and emit a
/// [`FileChangedOnDisk`] for each surviving path.
fn handle_event(
    ev: &DebouncedEvent,
    state: &Mutex<SharedState>,
    command_tx: &mpsc::Sender<Command>,
    emit_tx: &SyncSender<Event>,
) {
    let event = &ev.event;
    // notify emits `Access` (IN_OPEN/IN_ACCESS) and `Other` (attribute/chmod)
    // events that carry no content or name change. Go's fsnotify never surfaces
    // these (its op set is Create/Write/Remove/Rename/Chmod), so drop them to
    // match Go behavior and avoid spurious events on watched directories.
    if matches!(event.kind, EventKind::Access(_) | EventKind::Other) {
        return;
    }
    let now = Instant::now();

    for path in &event.paths {
        // Directory transitions are not file changes to surface. Some notify
        // backends report a newly created directory as a non-Create event, so
        // classify existing directory paths before inspecting the event kind.
        // Keep newly discovered directories watched for recursive coverage.
        // Use `symlink_metadata` so a newly created symlink to an external dir
        // (e.g. `ln -s /etc proj/link`) is rejected rather than watched.
        if let Ok(meta) = fs::symlink_metadata(path) {
            let ft = meta.file_type();
            if ft.is_symlink() && matches!(event.kind, EventKind::Create(_)) {
                continue;
            }
            if ft.is_dir() {
                let name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
                if !is_ignored_dir_name(name) && !name.starts_with('.') {
                    let _ = command_tx.send(Command::WatchNewDir(path.clone()));
                }
                continue;
            }
        }

        // Skip anything inside (or named as) an ignored directory.
        if path_has_ignored_component(path) {
            continue;
        }

        // Resolve owning workspace + suppression + throttle under one lock.
        let (ws_id, root) = {
            let mut s = state.lock().unwrap_or_else(PoisonError::into_inner);
            if s.closed {
                return;
            }
            // Opportunistic cleanup bounds the suppression/throttle maps.
            if now.duration_since(s.last_cleanup) > CLEANUP_INTERVAL {
                s.cleanup(now);
            }
            // Resolve the owning workspace root.
            let mut found: Option<(String, PathBuf)> = None;
            for (id, r) in &s.roots {
                if path_under_root(path, r) {
                    found = Some((id.clone(), r.clone()));
                    break;
                }
            }
            let Some((ws_id, root)) = found else {
                return;
            };
            // Suppress the app's own writes.
            if let Some(t) = s.app_writes.peek(path.as_path()) {
                if now.duration_since(*t) < APP_WRITE_SUPPRESSION {
                    return;
                }
            }
            // Throttle repeat events for the same path.
            if let Some(t) = s.last_emit.peek(path.as_path()) {
                if now.duration_since(*t) < EMIT_THROTTLE {
                    return;
                }
            }
            s.last_emit.put(path.clone(), now);
            (ws_id, root)
        };

        // Compute the workspace-relative path (forward slashes, like Go).
        let Ok(rel) = path.strip_prefix(&root) else {
            continue;
        };
        let rel_str = rel.to_string_lossy().replace('\\', "/");

        let mut event = Event::new(
            0,
            EventType::FileChangedOnDisk,
            String::new(),
            go_zero_time(),
        );
        event.workspace_id = ws_id;
        event.target = rel_str;

        // Drop the event if the emit channel is full rather than blocking the
        // watcher thread (matches Go's `default:` drop).
        if emit_tx.try_send(event).is_err() {
            debug!("fswatch: emit channel full, dropping event");
        }
    }
}

// ---------------------------------------------------------------------------
// Path helpers
// ---------------------------------------------------------------------------

/// Whether `name` is an ignored directory name.
fn is_ignored_dir_name(name: &str) -> bool {
    IGNORE_DIRS.contains(&name)
}

/// Whether any path component is an ignored directory. Mirrors Go
/// `pathHasIgnoredComponent`.
fn path_has_ignored_component(path: &Path) -> bool {
    path.components().any(|c| match c {
        std::path::Component::Normal(s) => s.to_str().is_some_and(is_ignored_dir_name),
        _ => false,
    })
}

/// Whether `path` is `root` or under `root`. Case-insensitive on Windows and
/// macOS (case-insensitive filesystems), case-sensitive elsewhere. Mirrors Go
/// `pathEqual` / `hasPathPrefix`.
fn path_under_root(path: &Path, root: &Path) -> bool {
    if cfg!(target_os = "windows") || cfg!(target_os = "macos") {
        let p = path.to_string_lossy().to_ascii_lowercase();
        let r = root.to_string_lossy().to_ascii_lowercase();
        p == r || p.starts_with(&format!("{r}/")) || p.starts_with(&format!("{r}\\"))
    } else {
        path == root || path.starts_with(root)
    }
}

#[cfg(test)]
mod tests;
