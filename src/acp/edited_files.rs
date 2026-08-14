//! Session-scoped cache of agent-edited file content, for the edited-files
//! popup's diff viewer and revert action. Cleared when a session closes.
//!
//! The cache stores the pre-edit content (for diff base), the revision written
//! by the agent (for conflict-safe revert), and line-count delta (for the
//! popup's +/- display) per (session, path) pair. Entry and content-size caps
//! bound memory even when an untrusted agent writes many or very large files.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// Maximum files retained for one session. Writes still succeed beyond this
/// limit, but those files do not get diff/revert actions.
const MAX_FILES_PER_SESSION: usize = 128;
/// Maximum size of either diff side considered cacheable. This caps retained
/// content and avoids allocating large line-frequency maps for the summary.
const MAX_DIFF_CONTENT_BYTES: usize = 1024 * 1024;
/// Aggregate retained pre-edit content across all live sessions.
const MAX_TOTAL_CONTENT_BYTES: usize = 64 * 1024 * 1024;
/// Aggregate metadata bound, including zero-byte pre-images.
const MAX_TOTAL_ENTRIES: usize = 4_096;
/// Prevent oversized untrusted path strings from dominating cache metadata.
const MAX_CACHED_PATH_BYTES: usize = 4 * 1024;

/// One cached edited-file entry: pre-edit content + line-count delta.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditedFileEntry {
    /// File content before the agent's latest write (diff base).
    pub content_before: String,
    /// Lines added by the agent's write (vs `content_before`).
    pub added_lines: u32,
    /// Lines removed by the agent's write (vs `content_before`).
    pub removed_lines: u32,
    /// Whether the path existed before the agent write. A false value makes
    /// revert delete the created file rather than leave an empty placeholder.
    pub existed_before: bool,
    /// Revision of the content written by the agent. Revert supplies this as
    /// `expected_revision` so later user or external edits are never clobbered.
    pub written_revision: i64,
    /// Best-effort write time used when the durable event has fallen outside
    /// the bounded event-history lookup.
    pub recorded_at: DateTime<Utc>,
}

/// Thread-safe, session-scoped cache of agent-edited files.
///
/// Keyed by `(session_id, path)`. The latest write wins — a second agent write
/// to the same path replaces the entry with the new pre-edit content (the
/// content as it was just before the second write).
#[derive(Debug, Clone)]
pub struct EditedFileCache {
    inner: Arc<Mutex<HashMap<(String, String), EditedFileEntry>>>,
    /// Serializes the read→write→record sequence and accept/revert mutations.
    /// Agent writes are infrequent, and one bounded lock avoids an unbounded
    /// per-path lock map while making same-path cache snapshots deterministic.
    write_lock: Arc<AsyncMutex<()>>,
}

impl Default for EditedFileCache {
    fn default() -> Self {
        Self {
            inner: Arc::new(Mutex::new(HashMap::new())),
            write_lock: Arc::new(AsyncMutex::new(())),
        }
    }
}

impl EditedFileCache {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Serialize an agent write or edited-file mutation with other such work.
    pub(crate) async fn lock_writes(&self) -> OwnedMutexGuard<()> {
        Arc::clone(&self.write_lock).lock_owned().await
    }

    /// Record a write: store the pre-edit content, written revision, and
    /// line-count delta. Overwrites a prior entry for the same key.
    ///
    /// Returns `false` when the content or per-session entry cap prevents
    /// caching. The underlying file write has already succeeded in that case.
    #[must_use]
    pub fn record(
        &self,
        session_id: &str,
        path: &str,
        content_before: String,
        content_after: &str,
        existed_before: bool,
        written_revision: i64,
    ) -> bool {
        if path.len() > MAX_CACHED_PATH_BYTES
            || content_before.len() > MAX_DIFF_CONTENT_BYTES
            || content_after.len() > MAX_DIFF_CONTENT_BYTES
        {
            return false;
        }

        let key = (session_id.to_string(), path.to_string());
        let mut entries = self.inner.lock();
        let is_existing = entries.contains_key(&key);
        if !is_existing && entries.len() >= MAX_TOTAL_ENTRIES {
            return false;
        }
        let replaced_bytes = entries
            .get(&key)
            .map_or(0, |entry| entry.content_before.len());
        let retained_bytes = entries
            .values()
            .fold(0_usize, |total, entry| {
                total.saturating_add(entry.content_before.len())
            })
            .saturating_sub(replaced_bytes);
        if retained_bytes.saturating_add(content_before.len()) > MAX_TOTAL_CONTENT_BYTES {
            return false;
        }
        if !is_existing
            && entries
                .keys()
                .filter(|(cached_session, _)| cached_session == session_id)
                .count()
                >= MAX_FILES_PER_SESSION
        {
            return false;
        }

        let (added, removed) = line_count_diff(&content_before, content_after);
        entries.insert(
            key,
            EditedFileEntry {
                content_before,
                added_lines: added,
                removed_lines: removed,
                existed_before,
                written_revision,
                recorded_at: Utc::now(),
            },
        );
        true
    }

    /// Get the cached entry for a (session, path) pair, if any.
    #[must_use]
    pub fn get(&self, session_id: &str, path: &str) -> Option<EditedFileEntry> {
        self.inner
            .lock()
            .get(&(session_id.to_string(), path.to_string()))
            .cloned()
    }

    /// Lightweight summaries for all cached files in a session.
    ///
    /// Pre-edit content is deliberately not cloned here: listing a session
    /// should stay cheap even near the cache's byte cap.
    #[must_use]
    pub fn list(&self, session_id: &str) -> Vec<(String, u32, u32, DateTime<Utc>)> {
        self.inner
            .lock()
            .iter()
            .filter(|((sid, _), _)| sid == session_id)
            .map(|((_, path), entry)| {
                (
                    path.clone(),
                    entry.added_lines,
                    entry.removed_lines,
                    entry.recorded_at,
                )
            })
            .collect()
    }

    /// Remove a single (session, path) entry. Returns true if it existed.
    #[must_use]
    pub fn remove(&self, session_id: &str, path: &str) -> bool {
        self.inner
            .lock()
            .remove(&(session_id.to_string(), path.to_string()))
            .is_some()
    }

    /// Clear all entries for a session, ordered after any in-flight agent write.
    pub async fn clear_session(&self, session_id: &str) {
        let _write_guard = self.lock_writes().await;
        self.inner.lock().retain(|(sid, _), _| sid != session_id);
    }
}

/// Compute an order-insensitive line multiset delta between two strings.
///
/// This is intentionally linear rather than a quadratic LCS diff because the
/// result is only a compact popup summary. Unlike a set comparison, frequency
/// maps still account correctly for duplicate lines.
fn line_count_diff(before: &str, after: &str) -> (u32, u32) {
    let mut frequencies: HashMap<&str, i64> = HashMap::new();
    for line in before.lines() {
        *frequencies.entry(line).or_default() += 1;
    }
    for line in after.lines() {
        *frequencies.entry(line).or_default() -= 1;
    }
    let removed = frequencies
        .values()
        .filter(|delta| **delta > 0)
        .fold(0_u32, |total, delta| {
            total.saturating_add(u32::try_from(*delta).unwrap_or(u32::MAX))
        });
    let added = frequencies
        .values()
        .filter(|delta| **delta < 0)
        .fold(0_u32, |total, delta| {
            total.saturating_add(u32::try_from(delta.saturating_abs()).unwrap_or(u32::MAX))
        });
    (added, removed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_count_diff_counts_added_and_removed() {
        let before = "a\nb\nc";
        let after = "a\nx\ny";
        let (added, removed) = line_count_diff(before, after);
        assert_eq!(added, 2); // x, y
        assert_eq!(removed, 2); // b, c
    }

    #[test]
    fn line_count_diff_identical_content_is_zero() {
        let (added, removed) = line_count_diff("a\nb", "a\nb");
        assert_eq!((added, removed), (0, 0));
    }

    #[test]
    fn line_count_diff_new_file_counts_all_as_added() {
        let (added, removed) = line_count_diff("", "a\nb\nc");
        assert_eq!(added, 3);
        assert_eq!(removed, 0);
    }

    #[test]
    fn line_count_diff_accounts_for_duplicate_lines() {
        assert_eq!(line_count_diff("a\na\nb", "a\nc"), (1, 2));
    }

    #[test]
    fn cache_record_and_get() {
        let cache = EditedFileCache::new();
        assert!(cache.record("s1", "src/main.rs", "old".to_string(), "new", true, 1));
        let entry = cache.get("s1", "src/main.rs").expect("entry");
        assert_eq!(entry.content_before, "old");
        assert_eq!(entry.written_revision, 1);
    }

    #[test]
    fn cache_latest_write_wins() {
        let cache = EditedFileCache::new();
        assert!(cache.record("s1", "file.rs", "v1".to_string(), "v2", true, 1));
        assert!(cache.record("s1", "file.rs", "v2".to_string(), "v3", true, 2));
        let entry = cache.get("s1", "file.rs").expect("entry");
        assert_eq!(entry.content_before, "v2");
    }

    #[test]
    fn cache_list_returns_session_entries_only() {
        let cache = EditedFileCache::new();
        assert!(cache.record("s1", "a.rs", "old".to_string(), "new", true, 1));
        assert!(cache.record("s2", "b.rs", "old".to_string(), "new", true, 1));
        let s1 = cache.list("s1");
        assert_eq!(s1.len(), 1);
        assert_eq!(s1[0].0, "a.rs");
    }

    #[tokio::test]
    async fn cache_clear_session_removes_only_that_session() {
        let cache = EditedFileCache::new();
        assert!(cache.record("s1", "a.rs", "old".to_string(), "new", true, 1));
        assert!(cache.record("s2", "b.rs", "old".to_string(), "new", true, 1));
        cache.clear_session("s1").await;
        assert!(cache.get("s1", "a.rs").is_none());
        assert!(cache.get("s2", "b.rs").is_some());
    }

    #[test]
    fn cache_rejects_oversized_content() {
        let cache = EditedFileCache::new();
        assert!(!cache.record(
            "s1",
            "large.txt",
            "x".repeat(MAX_DIFF_CONTENT_BYTES + 1),
            "small",
            true,
            1,
        ));
        assert!(cache.get("s1", "large.txt").is_none());
    }

    #[test]
    fn cache_caps_new_paths_per_session_but_allows_updates() {
        let cache = EditedFileCache::new();
        for index in 0..MAX_FILES_PER_SESSION {
            assert!(cache.record("s1", &format!("{index}.txt"), String::new(), "x", false, 1));
        }
        assert!(!cache.record("s1", "overflow.txt", String::new(), "x", false, 1));
        assert!(cache.record("s1", "0.txt", "x".to_string(), "y", true, 2));
        assert_eq!(cache.list("s1").len(), MAX_FILES_PER_SESSION);
    }

    #[test]
    fn cache_caps_total_metadata_for_zero_byte_preimages() {
        let cache = EditedFileCache::new();
        let sessions = MAX_TOTAL_ENTRIES / MAX_FILES_PER_SESSION;
        for session in 0..sessions {
            for file in 0..MAX_FILES_PER_SESSION {
                assert!(cache.record(
                    &format!("session-{session}"),
                    &format!("{file}.txt"),
                    String::new(),
                    "x",
                    false,
                    1,
                ));
            }
        }
        assert!(!cache.record(
            "overflow-session",
            "overflow.txt",
            String::new(),
            "x",
            false,
            1,
        ));
    }
}
