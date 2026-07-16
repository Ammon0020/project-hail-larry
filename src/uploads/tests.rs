//! Tests for the upload store (port of `internal/uploads/uploads_test.go`).
//!
//! Uses `tempfile` so tests never touch the developer's real
//! `~/.local-agent/uploads/` directory.

use std::io::Cursor;

use super::*;

/// A valid 1x1 PNG header + IHDR chunk — enough for magic-byte detection
/// without being a fully decodable image. Mirrors Go `minimalPNG`.
fn minimal_png() -> Vec<u8> {
    let mut v = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    v.extend(std::iter::repeat_n(0, 32));
    v
}

#[test]
fn store_and_detect() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");

    let png = minimal_png();
    let stored = m
        .store("sess-1", "photo.PNG", &mut Cursor::new(&png))
        .expect("store");

    assert_eq!(stored.mime_type, "image/png");
    assert!(!stored.id.is_empty());
    assert_eq!(stored.id.len(), 32, "id should be 32 hex chars");
    assert!(
        stored.path.is_absolute(),
        "path {} is not absolute",
        stored.path.display()
    );
    assert_eq!(stored.uri, format!("file://{}", stored.path.display()));
    // The display name keeps the original extension but the on-disk file uses
    // the upload ID with the magic-byte-derived extension.
    assert_eq!(stored.name, "photo.PNG");
    assert_eq!(
        stored.path.extension().and_then(|e| e.to_str()),
        Some("png"),
        "on-disk ext should be .png"
    );
    assert!(stored.path.exists(), "stored file missing");
    assert_eq!(stored.size, png.len() as u64);
}

#[test]
fn store_rejects_unsupported() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let err = m
        .store("s", "f.txt", &mut Cursor::new(b"hello world".to_vec()))
        .unwrap_err();
    assert!(matches!(err, UploadError::UnsupportedType), "got {err:?}");
}

#[test]
fn store_rejects_oversize() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    // Build a fake "PNG" that exceeds the limit.
    let mut big = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A];
    big.extend(std::iter::repeat_n(0, MAX_UPLOAD_BYTES + 10));
    let err = m.store("s", "big.png", &mut Cursor::new(big)).unwrap_err();
    assert!(matches!(err, UploadError::Oversize(_)), "got {err:?}");
}

#[test]
fn get_round_trip() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    let stored = m
        .store("sess", "x.png", &mut Cursor::new(&png))
        .expect("store");
    let got = m.get("sess", &stored.id).expect("get");
    assert_eq!(got, stored.path);
}

#[test]
fn get_rejects_bad_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let m = Manager::new(tmp.path()).expect("new");
    let err = m.get("s", "../escape").unwrap_err();
    assert!(matches!(err, UploadError::InvalidUploadId), "got {err:?}");
    let err = m.get("s", "tooshort").unwrap_err();
    assert!(matches!(err, UploadError::InvalidUploadId), "got {err:?}");
}

#[test]
fn remove_session() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    m.store("s", "x.png", &mut Cursor::new(&png))
        .expect("store");
    let dir = m.root().join("s");
    assert!(dir.exists(), "session dir missing before remove");

    m.remove_session("s").expect("remove_session");
    assert!(!dir.exists(), "session dir still exists after remove");

    // Removing a session with no uploads is a no-op (not an error).
    m.remove_session("never-existed")
        .expect("remove on missing dir should be no-op");
}

#[test]
fn store_rejects_invalid_session_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    // Empty session id.
    let err = m.store("", "x.png", &mut Cursor::new(&png)).unwrap_err();
    assert!(matches!(err, UploadError::EmptySessionId), "got {err:?}");
    // Traversal attempt.
    let err = m
        .store("../escape", "x.png", &mut Cursor::new(&png))
        .unwrap_err();
    assert!(matches!(err, UploadError::InvalidSessionId), "got {err:?}");
    // Path separator.
    let err = m.store("a/b", "x.png", &mut Cursor::new(&png)).unwrap_err();
    assert!(matches!(err, UploadError::InvalidSessionId), "got {err:?}");
}

#[test]
fn store_serves_correct_content_type_for_each_format() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");

    // Minimal GIF header (GIF87a) — enough for infer's magic-byte detection.
    let gif = b"GIF87a\x01\x00\x01\x00\x80\x00\x00".to_vec();
    let stored = m
        .store("s", "a.gif", &mut Cursor::new(&gif))
        .expect("store gif");
    assert_eq!(stored.mime_type, "image/gif");
    assert_eq!(
        stored.path.extension().and_then(|e| e.to_str()),
        Some("gif")
    );

    // Minimal JPEG: FFD8FF (SOI + start-of-marker) + a few bytes.
    let jpeg = [0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F'].to_vec();
    let stored = m
        .store("s", "a.jpeg", &mut Cursor::new(&jpeg))
        .expect("store jpeg");
    assert_eq!(stored.mime_type, "image/jpeg");
    // Go uses .jpg (not .jpeg) as the canonical extension.
    assert_eq!(
        stored.path.extension().and_then(|e| e.to_str()),
        Some("jpg")
    );
}

#[test]
fn store_id_is_opaque_and_unique() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    let a = m
        .store("s", "a.png", &mut Cursor::new(&png))
        .expect("store a");
    let b = m
        .store("s", "b.png", &mut Cursor::new(&png))
        .expect("store b");
    assert_ne!(a.id, b.id, "upload IDs must be unique");
    // IDs must be valid per is_valid_id (32 lowercase hex chars).
    assert!(is_valid_id(&a.id));
    assert!(is_valid_id(&b.id));
}

#[test]
fn get_rejects_invalid_session_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let m = Manager::new(tmp.path()).expect("new");
    let err = m
        .get("../escape", "00000000000000000000000000000000")
        .unwrap_err();
    assert!(matches!(err, UploadError::InvalidSessionId), "got {err:?}");
}

#[test]
fn remove_session_rejects_invalid_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let m = Manager::new(tmp.path()).expect("new");
    let err = m.remove_session("../escape").unwrap_err();
    assert!(matches!(err, UploadError::InvalidSessionId), "got {err:?}");
}

#[test]
fn remove_all_clears_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    m.store("s1", "a.png", &mut Cursor::new(&png))
        .expect("store");
    m.store("s2", "b.png", &mut Cursor::new(&png))
        .expect("store");
    m.remove_all().expect("remove_all");
    assert!(!m.root().exists(), "root still exists after remove_all");
}

#[test]
fn sanitize_filename_strips_paths() {
    assert_eq!(sanitize_filename("photo.PNG"), "photo.PNG");
    assert_eq!(sanitize_filename("/etc/passwd"), "passwd");
    assert_eq!(sanitize_filename(".."), "upload");
    assert_eq!(sanitize_filename(""), "upload");
}

#[test]
fn is_valid_id_rejects_traversal() {
    assert!(is_valid_id("00000000000000000000000000000000"));
    assert!(!is_valid_id("../escape"));
    assert!(!is_valid_id("tooshort"));
    assert!(!is_valid_id("GGGGGGGGGGGGGGGGGGGGGGGGGGGGGGGG")); // non-hex
    assert!(!is_valid_id("0000000000000000000000000000000A")); // uppercase
}

#[test]
fn is_valid_session_id_rejects_traversal() {
    assert!(is_valid_session_id("sess-1"));
    assert!(is_valid_session_id("abc123"));
    assert!(!is_valid_session_id(""));
    assert!(!is_valid_session_id("."));
    assert!(!is_valid_session_id(".."));
    assert!(!is_valid_session_id("../escape"));
    assert!(!is_valid_session_id("a/b"));
    assert!(!is_valid_session_id("a\\b"));
    assert!(!is_valid_session_id("a\x00b"));
}

#[test]
fn store_creates_root_if_missing() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let root = tmp.path().join("nested").join("uploads");
    assert!(!root.exists());
    let _m = Manager::new(&root).expect("new creates root");
    assert!(root.exists());
}

#[test]
fn store_path_is_within_root() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    let stored = m
        .store("sess", "x.png", &mut Cursor::new(&png))
        .expect("store");
    // The stored path must start with the manager root + session id.
    let expected_prefix = m.root().join("sess");
    assert!(
        stored.path.starts_with(&expected_prefix),
        "path {} should be under {}",
        stored.path.display(),
        expected_prefix.display()
    );
}

#[test]
fn get_returns_not_found_for_unknown_id() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let m = Manager::new(tmp.path()).expect("new");
    let err = m.get("s", "00000000000000000000000000000000").unwrap_err();
    assert!(matches!(err, UploadError::NotFound { .. }), "got {err:?}");
}

#[test]
fn store_preserves_filename_display_only() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let mut m = Manager::new(tmp.path()).expect("new");
    let png = minimal_png();
    // A path-like filename should be sanitized to its base for display, while
    // the on-disk name is the upload ID.
    let stored = m
        .store("s", "/home/user/photo.PNG", &mut Cursor::new(&png))
        .expect("store");
    assert_eq!(stored.name, "photo.PNG");
    assert!(!stored
        .path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .contains("photo"));
}
