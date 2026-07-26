//! Revision tracking and three-way merge (Go `internal/files/`).
//!
//! Blueprint reference: Sec 14 (File System Access — Client File Sync).
//!
//! Every file has a monotonic revision number that increments on each write.
//! On save, the client sends content plus `expected_revision`. If revisions
//! match, the host applies and broadcasts. If stale, [`AppError::StaleRevision`]
//! is returned — Phase 1 does NOT attempt a server-side three-way merge (the
//! frontend uses `@codemirror/merge` for the merge UI). When/if server-side
//! merge is added later, the `similar`/`diffy` crates should be used and the
//! reconciliation logic kept thin and fixture-tested.
//!
//! # Concurrency model
//!
//! The Go daemon originally held a single global mutex across disk I/O,
//! serializing all workspaces. Instead, [`FileSync`] uses a **per-file lock**
//! (keyed by `workspace_id/rel_path`) so concurrent saves to different files do
//! not block each other, plus a short-lived map lock that is only held for the
//! brief map read/write — never across disk I/O. The base-content cache is a
//! bounded LRU ([`lru::LruCache`], 256 entries).
//!
//! Per-file lock entries are [`DashMap`] entries mapping to
//! `Arc<tokio::sync::Mutex<()>>`. To prevent the lock map from growing
//! indefinitely with arbitrary file paths, idle entries are evicted after the
//! per-file operation completes (see [`FileSync::lock_for`]).
//!
//! # 48-bit content hash
//!
//! [`content_revision`] computes a deterministic revision from file content by
//! taking the leading 48 bits of the SHA-256 digest as a positive `i64`. It
//! changes whenever the content changes and is stable across reads of identical
//! content, making it suitable for optimistic-lock comparisons. The width is
//! capped at 48 bits (rather than 64) so the value fits within JavaScript's
//! `Number.MAX_SAFE_INTEGER` (2^53-1): the frontend round-trips the revision
//! through `JSON.parse`/`JSON.stringify`, which use IEEE-754 doubles and
//! silently round integers beyond 2^53. This function is preserved exactly from
//! the Go `internal/workspace.contentRevision` for future use by the workspace
//! manager (S-WORKSPACE).

#[cfg(test)]
mod tests;

use std::collections::HashMap;
use std::num::NonZeroUsize;
use std::path::Path;
use std::sync::Arc;

use dashmap::DashMap;
use lru::LruCache;
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::interfaces::traits::FileSync as FileSyncTrait;
use crate::interfaces::AppError;
use crate::pathutil::{clean_path, resolve_symlink};

/// Maximum number of entries in the base-content LRU cache. The daemon is
/// long-lived and may touch thousands of files; without a bound this cache
/// would grow without limit. Eviction is LRU: the least-recently-used entry is
/// dropped when the cache is full. Matches Go `maxContentsEntries`.
const MAX_CONTENTS_ENTRIES: usize = 256;

/// File sync and revision tracking (Go `internal/files.FileSync`).
///
/// Per-file locks ([`DashMap`] of `Arc<tokio::sync::Mutex<()>>`) serialize the
/// check-write-update sequence for a single file while allowing different files
/// to proceed concurrently. The revisions map and the LRU base-content cache
/// are guarded by a short-lived [`Mutex`] (`map_mu`) that is only held for map
/// operations — never across disk I/O.
pub struct FileSync {
    /// Per-file mutexes keyed by `workspace_id/rel_path`. Each entry is an
    /// `Arc<Mutex<()>>` so the lock can be held after the `DashMap` entry is
    /// dropped. Idle entries are evicted by [`Self::lock_for`] after the
    /// operation completes to prevent unbounded growth.
    locks: DashMap<String, Arc<Mutex<()>>>,

    /// Current revision per file key (`workspace_id/rel_path` → revision).
    revisions: Mutex<HashMap<String, i64>>,

    /// Bounded LRU cache of last-known file content, used as the three-way
    /// merge base. Evicted on access so it cannot grow unbounded.
    contents: Mutex<LruCache<String, String>>,
}

impl FileSync {
    /// Create a new [`FileSync`] with the default 256-entry LRU cache.
    #[must_use]
    pub fn new() -> Self {
        Self::with_capacity(MAX_CONTENTS_ENTRIES)
    }

    /// Create a new [`FileSync`] with a custom LRU cache capacity (mainly for
    /// testing).
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        // max(1) guarantees non-zero; unwrap is safe but clippy::expect_used
        // denies expect(), so use unwrap_or to avoid the panic path.
        let cap = NonZeroUsize::new(capacity.max(1)).unwrap_or(NonZeroUsize::MIN);
        Self {
            locks: DashMap::new(),
            revisions: Mutex::new(HashMap::new()),
            contents: Mutex::new(LruCache::new(cap)),
        }
    }

    /// Returns the per-file mutex for the given key, creating one on first use.
    ///
    /// The locks `DashMap` itself is concurrency-safe; the returned `Arc<Mutex>`
    /// is then held by the caller for the duration of the per-file operation
    /// (including disk I/O), which serializes operations on the same file only.
    ///
    /// **GC:** after acquiring the lock, this method attempts to evict the
    /// `DashMap` entry if no other reference to the `Arc` exists. This prevents
    /// the lock map from growing indefinitely with arbitrary file paths. If
    /// eviction fails (another caller raced and grabbed the same `Arc`), the
    /// entry stays for the next operation to find.
    fn lock_for(&self, key: &str) -> Arc<Mutex<()>> {
        // Fast path: entry already exists — clone the Arc and return.
        if let Some(entry) = self.locks.get(key) {
            return Arc::clone(&entry);
        }
        // Slow path: create a new entry. DashMap's entry API handles the
        // race between two callers both seeing "no entry" — only one wins
        // the insert; the other gets the winner's Arc.
        let entry = self
            .locks
            .entry(key.to_string())
            .or_insert_with(|| Arc::new(Mutex::new(())));
        Arc::clone(&entry)
    }

    /// Evict the per-file lock entry for `key` if no other task is waiting on
    /// it. Called after the per-file operation completes. This is best-effort:
    /// if eviction fails (another caller grabbed the Arc between the lock
    /// release and this call), the entry remains for the next caller.
    ///
    /// We check `Arc::strong_count == 1` (only the `DashMap` holds it) before
    /// removing. This is inherently racy but safe: worst case the entry stays
    /// (a minor memory cost) or a new caller re-creates it (no correctness
    /// impact).
    fn gc_lock(&self, key: &str) {
        // Try to remove only if we're the sole holder. DashMap's remove_if
        // atomically checks a predicate under the shard lock.
        self.locks.remove_if(key, |_, arc| {
            // strong_count == 1 means only the DashMap holds the Arc; no
            // other task has a clone, so it's safe to evict.
            Arc::strong_count(arc) == 1
        });
    }

    /// Read the current revision for a key under the brief map mutex.
    /// Returns `(revision, exists)`.
    async fn current_rev(&self, key: &str) -> (i64, bool) {
        let revisions = self.revisions.lock().await;
        revisions
            .get(key)
            .copied()
            .map_or((0, false), |rev| (rev, true))
    }

    /// Set the revision for a key under the brief map mutex.
    async fn set_rev(&self, key: &str, rev: i64) {
        let mut revisions = self.revisions.lock().await;
        revisions.insert(key.to_string(), rev);
    }

    /// Write file content with optimistic locking via `expected_revision`.
    ///
    /// Returns the new revision on success. Returns [`AppError::StaleRevision`]
    /// if the file has been modified since the client last read it.
    ///
    /// In Phase 1, a stale revision returns [`AppError::StaleRevision`] without
    /// attempting a three-way merge. The merge UI is handled by the frontend
    /// using `@codemirror/merge`.
    ///
    /// The revision check and increment happen under a per-file mutex so the
    /// optimistic-lock check is atomic per file, while disk I/O for one file
    /// does not block concurrent saves to a different file.
    ///
    /// # Arguments
    /// * `workspace_path` - Absolute path to the workspace root. In the Go
    ///   daemon, the workspace manager resolves `workspace_id` → path before
    ///   calling `FileSync`. The [`FileSyncTrait`] passes `workspace_id`; this
    ///   implementation treats it as the workspace path (the manager will
    ///   resolve IDs to paths in S-WORKSPACE).
    /// * `rel_path` - Relative path within the workspace.
    /// * `content` - New file content.
    /// * `expected_revision` - Revision the client last saw. `0` for a new
    ///   file (no prior revision expected).
    async fn save_inner(
        &self,
        workspace_path: &str,
        rel_path: &str,
        content: &str,
        expected_revision: i64,
    ) -> Result<i64, AppError> {
        // Defense-in-depth: the workspace manager always resolves workspace_id to
        // a canonical absolute root before calling FileSync. A raw client string
        // must never reach clean_path as a root. Catch mis-wiring in debug builds.
        debug_assert!(
            Path::new(workspace_path).is_absolute(),
            "workspace_path must be an absolute resolved root, got {workspace_path:?}"
        );
        let key = file_key(workspace_path, rel_path);
        let lock = self.lock_for(&key);
        // Move the owned guard into the blocking task. If this async future is
        // dropped while awaiting that task, the task continues to own the
        // guard until its disk write has stopped mutating the file.
        let guard = Arc::clone(&lock).lock_owned().await;

        // Read the current revision under the brief map mutex.
        let (current_rev, exists) = self.current_rev(&key).await;

        if exists && current_rev != expected_revision {
            // Stale: another write raced ahead. Phase 1 returns without
            // attempting a server-side merge.
            return Err(AppError::StaleRevision);
        }

        // Path resolution and disk I/O can block on networked or slow filesystems.
        // Keep the per-file mutex across the write for atomic revisions, but move
        // the blocking work off Tokio's async workers.
        let workspace_path = workspace_path.to_owned();
        let rel_path = rel_path.to_owned();
        let bytes = content.as_bytes().to_vec();
        let (write_result, guard) = tokio::task::spawn_blocking(move || {
            let write_result = (|| {
                let full_path = clean_path(Path::new(&workspace_path), &rel_path)?;
                // Layer symlink containment on top of the lexical check (defends
                // against agent-created symlinks that escape the workspace root).
                let full_path = resolve_symlink(Path::new(&workspace_path), &full_path)?;

                // Ensure parent directory exists (matches Go os.MkdirAll(dir, 0755)).
                if let Some(parent) = full_path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| AppError::Internal(format!("create dir: {e}")))?;
                }

                // Write file with 0644 permissions (user-editable by normal tools).
                std::fs::write(&full_path, bytes)
                    .map_err(|e| AppError::Internal(format!("write file: {e}")))?;

                Ok::<(), AppError>(())
            })();
            (write_result, guard)
        })
        .await
        .map_err(|e| AppError::Internal(format!("file write task: {e}")))?;
        write_result?;

        // Increment revision and update the base-content cache under the brief
        // map mutex. The per-file mutex guarantees no concurrent writer for
        // this key raced the check-and-update.
        let new_rev = if exists { current_rev + 1 } else { 1 };
        self.set_rev(&key, new_rev).await;

        {
            let mut contents = self.contents.lock().await;
            contents.put(key.clone(), content.to_string());
        }

        // Best-effort GC of the per-file lock entry. Both the guard and the
        // `Arc` clone (`lock`) must be dropped BEFORE checking the strong count,
        // otherwise `Arc::strong_count` is always ≥2 (DashMap + this local) and
        // `gc_lock`'s `remove_if` predicate never succeeds, leaking every
        // per-file lock entry. Dropping `lock` first leaves only the DashMap
        // holding the Arc, so `strong_count == 1` and the entry is evicted.
        drop(guard);
        drop(lock);
        self.gc_lock(&key);

        Ok(new_rev)
    }

    /// Returns the latest revision of a file. Returns `0` if the file has not
    /// been tracked yet.
    async fn current_revision_inner(
        &self,
        workspace_path: &str,
        rel_path: &str,
    ) -> Result<i64, AppError> {
        let key = file_key(workspace_path, rel_path);
        let (rev, _exists) = self.current_rev(&key).await;
        Ok(rev)
    }

    /// Registers a file in the revision tracker with its initial content.
    /// Called when a file is first read from disk. If the file is already
    /// tracked, this is a no-op (preserves the existing revision).
    pub async fn track_file(&self, workspace_path: &str, rel_path: &str, content: &str) {
        let key = file_key(workspace_path, rel_path);
        let (rev, exists) = self.current_rev(&key).await;
        if !exists {
            self.set_rev(&key, 1).await;
            let mut contents = self.contents.lock().await;
            contents.put(key, content.to_string());
        }
        // Suppress unused-variable warning when the revision exists.
        let _ = rev;
    }

    /// Returns the last known content for a file (used as merge base).
    /// Accessing it marks the entry as most-recently-used so the LRU policy
    /// keeps actively-merged files resident.
    pub async fn get_base_content(&self, workspace_path: &str, rel_path: &str) -> Option<String> {
        let key = file_key(workspace_path, rel_path);
        let mut contents = self.contents.lock().await;
        contents.get(&key).cloned()
    }

    /// Drops the cached base content for a file. Call this when a file is
    /// closed to release memory for files that no longer need a merge base.
    pub async fn forget(&self, workspace_path: &str, rel_path: &str) {
        let key = file_key(workspace_path, rel_path);
        let mut contents = self.contents.lock().await;
        contents.pop(&key);
    }

    /// Returns the number of entries currently in the LRU base-content cache.
    /// Mainly for testing.
    #[cfg(test)]
    pub async fn cache_len(&self) -> usize {
        let contents = self.contents.lock().await;
        contents.len()
    }

    /// Returns the number of per-file lock entries currently in the `DashMap`.
    /// Mainly for testing.
    #[cfg(test)]
    pub fn locks_len(&self) -> usize {
        self.locks.len()
    }
}

impl Default for FileSync {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait::async_trait]
impl FileSyncTrait for FileSync {
    /// Write file content with optimistic locking via `expected_revision`.
    /// Returns the new revision, or [`AppError::StaleRevision`] on conflict.
    ///
    /// `workspace_id` is treated as the workspace path (the workspace manager
    /// resolves IDs to paths before calling `FileSync` in the Go daemon; that
    /// resolution will be wired in S-WORKSPACE).
    async fn save(
        &self,
        workspace_id: &str,
        rel_path: &str,
        content: &str,
        expected_revision: i64,
    ) -> Result<i64, AppError> {
        self.save_inner(workspace_id, rel_path, content, expected_revision)
            .await
    }

    /// Latest revision of a file. Returns `0` if untracked.
    async fn current_revision(&self, workspace_id: &str, rel_path: &str) -> Result<i64, AppError> {
        self.current_revision_inner(workspace_id, rel_path).await
    }
}

/// Generates a unique key for a file within a workspace.
///
/// Mirrors Go `fileKey(workspacePath, relPath)` which uses `filepath.Join`.
/// We use a simple `/`-separated concatenation since the key is only used for
/// map lookups (not filesystem access) and must be deterministic.
pub(super) fn file_key(workspace_path: &str, rel_path: &str) -> String {
    // Normalize: trim trailing separators from workspace_path, trim leading
    // separators from rel_path, join with a single '/'.
    let ws = workspace_path.trim_end_matches('/');
    let rel = rel_path.trim_start_matches('/');
    format!("{ws}/{rel}")
}

/// Computes a deterministic revision from file content by taking the leading
/// 48 bits of the SHA-256 digest as a positive `i64`.
///
/// This is preserved exactly from Go `internal/workspace.contentRevision`. It
/// changes whenever the content changes and is stable across reads of
/// identical content, making it suitable for optimistic-lock comparisons.
///
/// The width is capped at 48 bits (rather than 64) so the value fits within
/// JavaScript's `Number.MAX_SAFE_INTEGER` (2^53-1): the frontend round-trips
/// the revision through `JSON.parse`/`JSON.stringify`, which use IEEE-754
/// doubles and silently round integers beyond 2^53. A full 64-bit revision
/// would be altered by that round trip and never match the backend's
/// recomputed hash, breaking every save. 48 bits (2^48 possible values) keeps
/// the collision probability negligible (~2^-48 per comparison) while
/// remaining exactly representable as a JS number.
///
/// The result is always in `[0, 2^48)`, so it is non-zero for any realistic
/// content (a SHA-256 whose first 6 bytes are all zero is astronomically
/// unlikely) and fits exactly in a JavaScript number.
#[must_use]
pub fn content_revision(content: &[u8]) -> i64 {
    let mut hasher = Sha256::new();
    hasher.update(content);
    let h = hasher.finalize();
    // Take the first 6 bytes (48 bits) and combine into a positive i64.
    // The result is always in [0, 2^48), well within i64 range.
    let bytes: &[u8] = h.as_ref();
    i64::from(bytes[0]) << 40
        | i64::from(bytes[1]) << 32
        | i64::from(bytes[2]) << 24
        | i64::from(bytes[3]) << 16
        | i64::from(bytes[4]) << 8
        | i64::from(bytes[5])
}
