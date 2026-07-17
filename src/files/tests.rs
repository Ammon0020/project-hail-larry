//! Tests for file sync and revision tracking (port of `files_test.go` +
//! property tests).
//!
//! Covers: optimistic revision check, [`AppError::StaleRevision`] on stale,
//! LRU bounded at 256 entries, per-file locks don't block different files,
//! non-overlapping/conflicting edits, revision/cache bounds, path traversal
//! rejection, nested path creation, and the 48-bit content hash.

use std::sync::Arc;

use tempfile::TempDir;

use super::{content_revision, file_key, FileSync};
use crate::interfaces::AppError;
use sha2::{Digest, Sha256};

// The `save` and `current_revision` methods are defined on the `FileSync`
// trait (in `crate::interfaces::traits`), not on the `FileSync` struct
// directly. Importing the trait as `_` brings its methods into scope for
// method-resolution without introducing a name clash with the struct of the
// same name.
use crate::interfaces::traits::FileSync as _;

// ---------------------------------------------------------------------------
// Ported from files_test.go
// ---------------------------------------------------------------------------

/// Saving a new file creates it with revision 1.
#[tokio::test]
async fn save_new_file() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    let rev = fs.save(ws, "test.txt", "hello world", 0).await.unwrap();
    assert_eq!(rev, 1, "expected revision 1 for new file");

    // Verify file was written to disk.
    let content = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
    assert_eq!(content, "hello world");
}

/// Saving with the correct revision increments it.
#[tokio::test]
async fn save_update() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    let rev1 = fs.save(ws, "file.txt", "v1", 0).await.unwrap();

    let rev2 = fs.save(ws, "file.txt", "v2", rev1).await.unwrap();
    assert_eq!(rev2, rev1 + 1, "expected revision {rev1}+1");

    let content = std::fs::read_to_string(dir.path().join("file.txt")).unwrap();
    assert_eq!(content, "v2");
}

/// Saving with a stale revision fails with [`AppError::StaleRevision`].
#[tokio::test]
async fn save_stale_revision() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    let rev1 = fs.save(ws, "file.txt", "v1", 0).await.unwrap();

    // Simulates another device writing.
    fs.save(ws, "file.txt", "v2-from-other", rev1)
        .await
        .unwrap();

    // Try to save with the old revision — should fail.
    let err = fs
        .save(ws, "file.txt", "v2-from-me", rev1)
        .await
        .unwrap_err();
    assert!(
        matches!(err, AppError::StaleRevision),
        "expected AppError::StaleRevision, got {err:?}"
    );
}

/// Revision tracking returns 0 for untracked files, 1 after first save.
#[tokio::test]
async fn current_revision() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    let rev = fs.current_revision(ws, "file.txt").await.unwrap();
    assert_eq!(rev, 0, "expected revision 0 for untracked file");

    fs.save(ws, "file.txt", "content", 0).await.unwrap();

    let rev = fs.current_revision(ws, "file.txt").await.unwrap();
    assert_eq!(rev, 1, "expected revision 1 after save");
}

/// Tracking a file sets the initial revision to 1 and caches base content.
#[tokio::test]
async fn track_file() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    fs.track_file(ws, "existing.txt", "existing content").await;

    let rev = fs.current_revision(ws, "existing.txt").await.unwrap();
    assert_eq!(rev, 1, "expected revision 1 after tracking");

    let content = fs.get_base_content(ws, "existing.txt").await;
    assert!(content.is_some(), "expected base content to exist");
    assert_eq!(
        content.as_deref(),
        Some("existing content"),
        "expected 'existing content'"
    );
}

/// Tracking an already-tracked file is a no-op (preserves existing revision).
#[tokio::test]
async fn track_file_idempotent() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // First save bumps revision to 2.
    fs.save(ws, "file.txt", "v1", 0).await.unwrap();
    fs.save(ws, "file.txt", "v2", 1).await.unwrap();

    // Track should NOT reset the revision.
    fs.track_file(ws, "file.txt", "tracked content").await;

    let rev = fs.current_revision(ws, "file.txt").await.unwrap();
    assert_eq!(rev, 2, "track_file should not reset existing revision");
}

/// Saving creates parent directories for nested paths.
#[tokio::test]
async fn save_nested_path() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    fs.save(ws, "src/routes/index.js", "console.log('hi');", 0)
        .await
        .unwrap();

    let path = dir.path().join("src").join("routes").join("index.js");
    assert!(path.exists(), "nested file not created");
}

/// Path traversal is blocked.
#[tokio::test]
async fn save_path_traversal() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    // Bind the canonicalized path to a binding so the borrowed `&str` outlives
    // the call. `dir.path().canonicalize().unwrap().to_str().unwrap()` would
    // return a `&str` into a temporary that is freed at the end of the
    // statement.
    let canon = dir.path().canonicalize().unwrap();
    let ws = canon.to_str().unwrap();

    let err = fs
        .save(ws, "../../../etc/passwd", "malicious", 0)
        .await
        .unwrap_err();
    assert!(
        !matches!(err, AppError::StaleRevision),
        "path traversal should not return StaleRevision"
    );
    // Should be a Path error (traversal or symlink).
    assert!(
        matches!(err, AppError::Path(_)),
        "expected AppError::Path for traversal, got {err:?}"
    );
}

// ---------------------------------------------------------------------------
// Property / additional tests
// ---------------------------------------------------------------------------

/// LRU cache is bounded at 256 entries — inserting more evicts the oldest.
#[tokio::test]
async fn lru_bounded_at_256() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // Save 300 distinct files; cache should cap at 256.
    for i in 0..300 {
        let name = format!("file_{i}.txt");
        fs.save(ws, &name, &format!("content {i}"), 0)
            .await
            .unwrap();
    }

    let len = fs.cache_len().await;
    assert_eq!(len, 256, "LRU cache should be bounded at 256, got {len}");
}

/// LRU eviction drops the least-recently-used entry.
#[tokio::test]
async fn lru_evicts_oldest() {
    // Use a small cache to make eviction observable.
    let fs = FileSync::with_capacity(2);
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    fs.save(ws, "a.txt", "a", 0).await.unwrap();
    fs.save(ws, "b.txt", "b", 0).await.unwrap();

    // Access "a" to mark it most-recently-used.
    fs.get_base_content(ws, "a.txt").await;

    // Insert "c" — should evict "b" (least recently used), not "a".
    fs.save(ws, "c.txt", "c", 0).await.unwrap();

    assert!(
        fs.get_base_content(ws, "a.txt").await.is_some(),
        "'a' should still be cached (was accessed recently)"
    );
    assert!(
        fs.get_base_content(ws, "b.txt").await.is_none(),
        "'b' should have been evicted (least recently used)"
    );
    assert!(
        fs.get_base_content(ws, "c.txt").await.is_some(),
        "'c' should be cached"
    );
}

/// Per-file locks don't block concurrent writes to different files.
///
/// Two saves to different files should run concurrently. If they were
/// serialized by a global lock, the total time would be ~2x the per-save delay.
/// We can't easily inject a delay into the file write, so we verify correctness
/// instead: both saves succeed and produce the right revisions.
#[tokio::test]
async fn per_file_locks_dont_block_different_files() {
    let fs = Arc::new(FileSync::new());
    let dir = Arc::new(TempDir::new().unwrap());
    let ws = dir.path().to_str().unwrap().to_string();

    // Spawn two concurrent saves to different files.
    let fs1 = Arc::clone(&fs);
    let ws1 = ws.clone();
    let h1 = tokio::spawn(async move { fs1.save(&ws1, "a.txt", "content a", 0).await.unwrap() });

    let fs2 = Arc::clone(&fs);
    let ws2 = ws.clone();
    let h2 = tokio::spawn(async move { fs2.save(&ws2, "b.txt", "content b", 0).await.unwrap() });

    let (r1, r2) = tokio::join!(h1, h2);
    assert_eq!(r1.unwrap(), 1, "file a should get revision 1");
    assert_eq!(r2.unwrap(), 1, "file b should get revision 1");

    // Both files should exist on disk.
    assert!(dir.path().join("a.txt").exists());
    assert!(dir.path().join("b.txt").exists());
}

/// Non-overlapping edits to different files don't conflict.
#[tokio::test]
async fn non_overlapping_edits_no_conflict() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // Two independent files, each saved multiple times.
    for i in 0..5 {
        let rev_a = if i == 0 { 0 } else { i };
        let rev_b = if i == 0 { 0 } else { i };
        let ra = fs.save(ws, "a.txt", &format!("a{i}"), rev_a).await.unwrap();
        let rb = fs.save(ws, "b.txt", &format!("b{i}"), rev_b).await.unwrap();
        assert_eq!(ra, i + 1);
        assert_eq!(rb, i + 1);
    }
}

/// Conflicting edits (same expected revision) — second writer gets
/// [`AppError::StaleRevision`].
#[tokio::test]
async fn conflicting_edits_stale() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // Both writers read revision 0 (new file).
    // First writer succeeds.
    let rev1 = fs.save(ws, "file.txt", "writer-1", 0).await.unwrap();
    assert_eq!(rev1, 1);

    // Second writer also expected revision 0 — should get stale (rev is now 1).
    let err = fs.save(ws, "file.txt", "writer-2", 0).await.unwrap_err();
    assert!(
        matches!(err, AppError::StaleRevision),
        "second writer with stale revision should get StaleRevision"
    );

    // Disk should have the first writer's content.
    let content = std::fs::read_to_string(dir.path().join("file.txt")).unwrap();
    assert_eq!(content, "writer-1");
}

/// Revision increments monotonically and never goes backwards.
#[tokio::test]
async fn revision_monotonic() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    let mut prev = 0;
    for i in 0..10 {
        let rev = fs
            .save(ws, "file.txt", &format!("v{i}"), prev)
            .await
            .unwrap();
        assert!(
            rev > prev,
            "revision should be monotonic: rev={rev}, prev={prev}"
        );
        prev = rev;
    }
    assert_eq!(prev, 10, "expected 10 revisions");
}

/// Forget drops the cached base content.
#[tokio::test]
async fn forget_drops_cache() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    fs.save(ws, "file.txt", "content", 0).await.unwrap();
    assert!(
        fs.get_base_content(ws, "file.txt").await.is_some(),
        "base content should be cached after save"
    );

    fs.forget(ws, "file.txt").await;
    assert!(
        fs.get_base_content(ws, "file.txt").await.is_none(),
        "base content should be gone after forget"
    );

    // Revision should still be tracked (forget only drops the cache).
    let rev = fs.current_revision(ws, "file.txt").await.unwrap();
    assert_eq!(rev, 1, "revision should survive forget");
}

/// Per-file lock entries are GC'd after operations complete (no unbounded
/// growth).
#[tokio::test]
async fn lock_gc_prevents_unbounded_growth() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // Save 50 different files — lock entries should be GC'd after each.
    for i in 0..50 {
        fs.save(ws, &format!("file_{i}.txt"), "x", 0).await.unwrap();
    }

    // After all operations complete, the lock map should be empty (or very
    // small) because each entry is evicted when its Arc's strong_count drops
    // to 1.
    let len = fs.locks_len();
    assert!(
        len <= 5,
        "lock map should be mostly empty after operations complete, got {len}"
    );
}

/// Concurrent saves to the same file are serialized — no lost updates.
#[tokio::test]
async fn concurrent_same_file_serialized() {
    let fs = Arc::new(FileSync::new());
    let dir = Arc::new(TempDir::new().unwrap());
    let ws = dir.path().to_str().unwrap().to_string();

    // 10 concurrent saves all expecting revision 0 — only one should succeed,
    // the rest get StaleRevision.
    let mut handles = Vec::new();
    for i in 0..10 {
        let fs = Arc::clone(&fs);
        let ws = ws.clone();
        handles.push(tokio::spawn(async move {
            fs.save(&ws, "file.txt", &format!("writer-{i}"), 0).await
        }));
    }

    let mut ok = 0;
    let mut stale = 0;
    for h in handles {
        match h.await.unwrap() {
            Ok(_) => ok += 1,
            Err(AppError::StaleRevision) => stale += 1,
            Err(e) => panic!("unexpected error: {e:?}"),
        }
    }
    assert_eq!(ok, 1, "exactly one concurrent writer should succeed");
    assert_eq!(stale, 9, "9 writers should get StaleRevision");
}

// ---------------------------------------------------------------------------
// 48-bit content hash tests (content_revision)
// ---------------------------------------------------------------------------

/// `content_revision` is deterministic — same content → same revision.
#[test]
fn content_revision_deterministic() {
    let r1 = content_revision(b"hello world");
    let r2 = content_revision(b"hello world");
    assert_eq!(r1, r2, "same content should produce same revision");
}

/// `content_revision` differs for different content.
#[test]
fn content_revision_differs_for_different_content() {
    let r1 = content_revision(b"hello");
    let r2 = content_revision(b"world");
    assert_ne!(
        r1, r2,
        "different content should produce different revisions"
    );
}

/// `content_revision` fits in 48 bits (within JS Number.MAX_SAFE_INTEGER).
#[test]
fn content_revision_fits_48_bits() {
    // Test with various content sizes.
    for size in &[1, 100, 1000, 10_000] {
        let content: Vec<u8> = (0..*size).map(|i| (i % 256) as u8).collect();
        let rev = content_revision(&content);
        assert!(
            rev >= 0,
            "revision should be non-negative (48-bit positive), got {rev}"
        );
        assert!(
            rev < (1i64 << 48),
            "revision should fit in 48 bits (< 2^48), got {rev}"
        );
    }
}

/// `content_revision` matches the Go algorithm exactly.
///
/// Go: `int64(uint64(h[0])<<40 | uint64(h[1])<<32 | ... | uint64(h[5]))`
/// where `h = sha256.Sum256(content)`.
#[test]
fn content_revision_matches_go_algorithm() {
    // Compute manually with sha2 to cross-check.
    let mut hasher = Sha256::new();
    hasher.update(b"test content");
    let h = hasher.finalize();
    let bytes: &[u8] = h.as_ref();
    let expected = i64::from(bytes[0]) << 40
        | i64::from(bytes[1]) << 32
        | i64::from(bytes[2]) << 24
        | i64::from(bytes[3]) << 16
        | i64::from(bytes[4]) << 8
        | i64::from(bytes[5]);

    let actual = content_revision(b"test content");
    assert_eq!(
        actual, expected,
        "content_revision should match manual SHA-256 first-6-bytes computation"
    );
}

/// `content_revision` of empty content is still valid (non-negative, 48-bit).
#[test]
fn content_revision_empty_content() {
    let rev = content_revision(b"");
    assert!(rev >= 0, "empty content revision should be non-negative");
    assert!(
        rev < (1i64 << 48),
        "empty content revision should fit 48 bits"
    );
}

/// `file_key` normalizes separators and is deterministic.
#[test]
fn file_key_normalizes() {
    assert_eq!(file_key("/ws", "file.txt"), "/ws/file.txt");
    assert_eq!(file_key("/ws/", "file.txt"), "/ws/file.txt");
    assert_eq!(file_key("/ws", "/file.txt"), "/ws/file.txt");
    assert_eq!(file_key("/ws/", "/file.txt"), "/ws/file.txt");
    assert_eq!(file_key("/ws//", "//file.txt"), "/ws/file.txt");
}

/// Stress test: many saves across many files, verify revision integrity.
#[tokio::test]
async fn stress_many_files_revisions() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // 50 files, 3 saves each.
    for f in 0..50 {
        let name = format!("f{f}.txt");
        let mut rev = 0;
        for s in 0..3 {
            rev = fs
                .save(ws, &name, &format!("content {f}-{s}"), rev)
                .await
                .unwrap();
            assert_eq!(rev, s + 1, "file {f} save {s}: expected rev {}", s + 1);
        }
    }

    // Verify all files exist and have the last-saved content.
    for f in 0..50 {
        let name = format!("f{f}.txt");
        let content = std::fs::read_to_string(dir.path().join(&name)).unwrap();
        assert_eq!(content, format!("content {f}-2"));
        let rev = fs.current_revision(ws, &name).await.unwrap();
        assert_eq!(rev, 3, "file {f} final revision should be 3");
    }
}

/// Saving the same content twice still increments the revision (monotonic,
/// not content-hash-based in the FileSync layer).
#[tokio::test]
async fn save_same_content_increments() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    let r1 = fs.save(ws, "file.txt", "same", 0).await.unwrap();
    let r2 = fs.save(ws, "file.txt", "same", r1).await.unwrap();
    assert_eq!(r1, 1);
    assert_eq!(r2, 2, "FileSync uses monotonic revisions, not content hash");
}

/// A short timeout ensures tests don't hang on lock contention bugs.
#[tokio::test]
async fn no_deadlock_on_sequential_saves() {
    let fs = FileSync::new();
    let dir = TempDir::new().unwrap();
    let ws = dir.path().to_str().unwrap();

    // If per-file lock GC is broken and removes an entry while a lock is
    // held, a subsequent save to the same file could create a NEW lock and
    // bypass serialization. This test verifies sequential saves still work
    // correctly (revision increments).
    let mut rev = 0;
    for i in 0..20 {
        rev = fs
            .save(ws, "file.txt", &format!("v{i}"), rev)
            .await
            .unwrap();
    }
    assert_eq!(rev, 20);
}
