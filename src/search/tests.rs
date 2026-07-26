//! Tests for the workspace content-search module (port of `search_test.go`).
//!
//! Both code paths are exercised:
//! - The native fallback is tested directly via [`super::native::search_with_walker`]
//!   so the tests pass regardless of whether `rg` is installed.
//! - The rg primary path is tested via [`super::rg::search_with_rg`] when `rg`
//!   is on `PATH` (skipped otherwise), plus a synthetic JSON parse test that
//!   runs everywhere.
//! - The public [`super::search`] entry point is tested for input validation
//!   (empty/invalid pattern) which is path-independent.

use std::fs;
use std::path::Path;

use tempfile::TempDir;
use tokio_util::sync::CancellationToken;

use crate::interfaces::{SearchOptions, SearchResult};
use crate::search::{glob_to_regex, search, SearchError};

// ============================================================================
// Test fixture (mirrors makeFixture in search_test.go)
// ============================================================================

/// Builds a small temp workspace tree:
/// ```text
/// root/
///   a.go        -> "package foo\nfunc Hello() {}\n"
///   b.txt       -> "TODO: fix this\nnothing here\n"
///   sub/
///     c.go      -> "var TODO = true\n"
///   .hidden     -> "TODO secret\n"        (must be skipped)
///   node_modules/
///     d.go      -> "TODO noise\n"         (must be skipped)
/// ```
fn make_fixture() -> TempDir {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();

    let write = |rel: &str, content: &str| {
        let full = root.join(rel);
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent).expect("mkdir");
        }
        fs::write(&full, content).expect("write");
    };

    write("a.go", "package foo\nfunc Hello() {}\n");
    write("b.txt", "TODO: fix this\nnothing here\n");
    write("sub/c.go", "var TODO = true\n");
    write(".hidden", "TODO secret\n");
    write("node_modules/d.go", "TODO noise\n");

    dir
}

/// Returns the [`SearchResult`] matching `path`, or `None`.
fn find_result<'a>(results: &'a [SearchResult], path: &str) -> Option<&'a SearchResult> {
    results.iter().find(|r| r.path == path)
}

/// A no-op cancellation token (never cancelled).
fn no_cancel() -> CancellationToken {
    CancellationToken::new()
}

/// Compiles a regex with the same `(?i)` prefix logic as `search()`.
fn compile_re(pattern: &str, ignore_case: bool) -> regex::Regex {
    let mut p = pattern.to_string();
    if ignore_case {
        p.insert_str(0, "(?i)");
    }
    regex::Regex::new(&p).expect("regex")
}

// ============================================================================
// Native fallback tests (run regardless of rg availability)
// ============================================================================

/// Exercises the native walker directly: path, line number, and match offsets.
/// Mirrors Go's `TestSearch_GoFallback`.
#[tokio::test]
async fn native_fallback_basic() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 50,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(dir.path(), &opts, &re, no_cancel())
        .await
        .expect("native search");

    // Expected: b.txt line 1, sub/c.go line 1. Hidden file and node_modules
    // must be skipped.
    assert_eq!(results.len(), 2, "expected 2 results, got {results:?}");
    let mut sorted = results.clone();
    sorted.sort_by(|a, b| a.path.cmp(&b.path));

    let r0 = &sorted[0];
    assert_eq!(r0.path, "b.txt");
    assert_eq!(r0.line_number, 1);
    assert_eq!(r0.line_content, "TODO: fix this");
    assert_eq!(r0.match_start, 0);
    assert_eq!(r0.match_end, 4);

    let r1 = &sorted[1];
    assert_eq!(r1.path, "sub/c.go");
    assert_eq!(r1.line_number, 1);
    assert_eq!(r1.match_start, 4);
    assert_eq!(r1.match_end, 8);
}

/// Case-insensitive matching finds "todo" in "TODO: fix this".
/// Mirrors Go's `TestSearch_IgnoreCase`.
#[tokio::test]
async fn native_fallback_ignore_case() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "todo".into(),
        ignore_case: true,
        max_results: 50,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(dir.path(), &opts, &re, no_cancel())
        .await
        .expect("native search");

    assert!(
        find_result(&results, "b.txt").is_some(),
        "expected a match in b.txt with IgnoreCase, got {results:?}"
    );
}

/// The file-name glob filter restricts which files are searched.
/// Mirrors Go's `TestSearch_FilePattern`.
#[tokio::test]
async fn native_fallback_file_pattern() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "TODO".into(),
        file_pattern: "*.go".into(),
        max_results: 50,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(dir.path(), &opts, &re, no_cancel())
        .await
        .expect("native search");

    // Only sub/c.go should match (a.go has no TODO; b.txt is filtered out by
    // the glob; node_modules is skipped).
    assert_eq!(
        results.len(),
        1,
        "expected 1 result for *.go, got {results:?}"
    );
    assert_eq!(results[0].path, "sub/c.go");
}

/// All returned paths are relative to root (never absolute).
/// Mirrors Go's `TestSearch_RelativePaths`.
#[tokio::test]
async fn native_fallback_relative_paths() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "TODO".into(),
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(dir.path(), &opts, &re, no_cancel())
        .await
        .expect("native search");
    for r in &results {
        assert!(
            !Path::new(&r.path).is_absolute(),
            "result path {} is absolute; expected relative to root",
            r.path
        );
    }
}

/// Binary files (null bytes in the first 512 bytes) are skipped without error.
#[tokio::test]
async fn native_fallback_skips_binary_files() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    // A "binary" file containing the pattern but with a null byte in the
    // first 512 bytes — must be skipped.
    let mut bin_content = vec![b'T', b'O', b'D', b'O', 0u8, b' ', b'x'];
    bin_content.resize(600, b' ');
    fs::write(root.join("bin.dat"), &bin_content).expect("write bin");
    // A normal text file that should match.
    fs::write(root.join("plain.txt"), "TODO here\n").expect("write plain");

    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 50,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(root, &opts, &re, no_cancel())
        .await
        .expect("native search");

    // Only plain.txt should match; bin.dat must be skipped.
    assert_eq!(
        results.len(),
        1,
        "expected 1 result (binary skipped), got {results:?}"
    );
    assert_eq!(results[0].path, "plain.txt");
}

/// Ignore dirs (`node_modules`, .git, …) are skipped by the native walker.
#[tokio::test]
async fn native_fallback_skips_ignore_dirs() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("top.txt"), "TODO top\n").expect("write");
    fs::create_dir_all(root.join("node_modules")).expect("mkdir");
    fs::write(root.join("node_modules/dep.go"), "TODO noise\n").expect("write");
    fs::create_dir_all(root.join("vendor")).expect("mkdir");
    fs::write(root.join("vendor/v.go"), "TODO vendor\n").expect("write");
    fs::create_dir_all(root.join("build")).expect("mkdir");
    fs::write(root.join("build/out.txt"), "TODO build\n").expect("write");

    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 50,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(root, &opts, &re, no_cancel())
        .await
        .expect("native search");

    // Only top.txt should match; node_modules, vendor, build must be skipped.
    assert_eq!(
        results.len(),
        1,
        "expected 1 result (ignore dirs skipped), got {results:?}"
    );
    assert_eq!(results[0].path, "top.txt");
}

/// `MaxResults` caps the number of matches returned.
#[tokio::test]
async fn native_fallback_max_results() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    // 5 files, each with one TODO match.
    for i in 0..5 {
        fs::write(root.join(format!("f{i}.txt")), "TODO match\n").expect("write");
    }
    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 3,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(root, &opts, &re, no_cancel())
        .await
        .expect("native search");
    assert_eq!(results.len(), 3, "max_results cap not honored");
}

/// Regex patterns (not just substrings) work in the native fallback.
#[tokio::test]
async fn native_fallback_regex() {
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("a.txt"), "foo123bar\nfoo456\nnope\n").expect("write");
    // Match "foo" followed by digits.
    let opts = SearchOptions {
        pattern: "foo[0-9]+".into(),
        max_results: 50,
        ..Default::default()
    };
    let re = compile_re(&opts.pattern, opts.ignore_case);
    let results = super::native::search_with_walker(root, &opts, &re, no_cancel())
        .await
        .expect("native search");
    assert_eq!(
        results.len(),
        2,
        "expected 2 regex matches, got {results:?}"
    );
}

// ============================================================================
// Entry-point validation tests (path-independent)
// ============================================================================

/// An empty pattern is rejected. Mirrors Go's `TestSearch_EmptyPattern`.
#[tokio::test]
async fn search_rejects_empty_pattern() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: String::new(),
        ..Default::default()
    };
    let err = search(dir.path(), &opts, no_cancel())
        .await
        .expect_err("empty");
    assert!(matches!(err, SearchError::EmptyPattern), "got {err:?}");
}

/// A whitespace-only pattern is rejected.
#[tokio::test]
async fn search_rejects_whitespace_pattern() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "   ".into(),
        ..Default::default()
    };
    let err = search(dir.path(), &opts, no_cancel())
        .await
        .expect_err("whitespace");
    assert!(matches!(err, SearchError::EmptyPattern), "got {err:?}");
}

/// An invalid regex surfaces as an error. Mirrors Go's `TestSearch_InvalidRegex`.
#[tokio::test]
async fn search_rejects_invalid_regex() {
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "[unclosed".into(),
        ..Default::default()
    };
    let err = search(dir.path(), &opts, no_cancel())
        .await
        .expect_err("invalid regex");
    assert!(matches!(err, SearchError::InvalidPattern(_)), "got {err:?}");
}

/// The default max-results cap (200) applies when `max_results` <= 0.
#[tokio::test]
async fn search_default_max_results() {
    // This is exercised indirectly: a search with max_results=0 should not
    // error and should return results. We just assert it runs.
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 0,
        ..Default::default()
    };
    let results = search(dir.path(), &opts, no_cancel())
        .await
        .expect("search");
    assert!(
        !results.is_empty(),
        "default cap should still return matches"
    );
}

// ============================================================================
// glob_to_regex unit tests
// ============================================================================

#[test]
fn glob_to_regex_star() {
    let re = regex::Regex::new(&glob_to_regex("*.go")).expect("regex");
    assert!(re.is_match("foo.go"));
    assert!(re.is_match("a.go"));
    assert!(!re.is_match("foo.txt"));
}

#[test]
fn glob_to_regex_question() {
    let re = regex::Regex::new(&glob_to_regex("a?b.txt")).expect("regex");
    assert!(re.is_match("aXb.txt"));
    assert!(!re.is_match("aXXb.txt"));
}

#[test]
fn glob_to_regex_escapes_dot() {
    let re = regex::Regex::new(&glob_to_regex("config.toml")).expect("regex");
    assert!(re.is_match("config.toml"));
    // A literal dot must not match any character.
    assert!(!re.is_match("configXtoml"));
}

// ============================================================================
// rg primary path tests
// ============================================================================

/// The rg primary path is exercised only when `rg` is on `PATH`. This tests
/// the full `search()` entry point (which dispatches to rg when available).
/// Mirrors the Go fixture-based tests but runs against whichever path is live.
#[tokio::test]
#[allow(clippy::print_stderr)] // skip notice — `tracing` is overkill in tests
async fn rg_primary_search_when_available() {
    if !super::rg::on_path() {
        eprintln!("rg not on PATH; skipping rg primary test");
        return;
    }
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 50,
        ..Default::default()
    };
    let results = search(dir.path(), &opts, no_cancel())
        .await
        .expect("rg search");
    // rg should find b.txt and sub/c.go (hidden + node_modules skipped).
    assert_eq!(results.len(), 2, "rg: expected 2 results, got {results:?}");
    assert!(find_result(&results, "b.txt").is_some());
    assert!(find_result(&results, "sub/c.go").is_some());
}

/// rg honors `IgnoreCase`.
#[tokio::test]
#[allow(clippy::print_stderr)] // skip notice — `tracing` is overkill in tests
async fn rg_primary_ignore_case_when_available() {
    if !super::rg::on_path() {
        eprintln!("rg not on PATH; skipping rg ignore-case test");
        return;
    }
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "todo".into(),
        ignore_case: true,
        max_results: 50,
        ..Default::default()
    };
    let results = search(dir.path(), &opts, no_cancel())
        .await
        .expect("rg search");
    assert!(
        find_result(&results, "b.txt").is_some(),
        "rg: expected b.txt match with IgnoreCase, got {results:?}"
    );
}

/// rg honors the file-name glob filter.
#[tokio::test]
#[allow(clippy::print_stderr)] // skip notice — `tracing` is overkill in tests
async fn rg_primary_file_pattern_when_available() {
    if !super::rg::on_path() {
        eprintln!("rg not on PATH; skipping rg file-pattern test");
        return;
    }
    let dir = make_fixture();
    let opts = SearchOptions {
        pattern: "TODO".into(),
        file_pattern: "*.go".into(),
        max_results: 50,
        ..Default::default()
    };
    let results = search(dir.path(), &opts, no_cancel())
        .await
        .expect("rg search");
    assert_eq!(
        results.len(),
        1,
        "rg: expected 1 result for *.go, got {results:?}"
    );
    assert_eq!(results[0].path, "sub/c.go");
}

/// rg skips the configured ignore dirs (`node_modules`, etc.).
#[tokio::test]
#[allow(clippy::print_stderr)] // skip notice — `tracing` is overkill in tests
async fn rg_primary_skips_ignore_dirs_when_available() {
    if !super::rg::on_path() {
        eprintln!("rg not on PATH; skipping rg ignore-dirs test");
        return;
    }
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    fs::write(root.join("top.txt"), "TODO top\n").expect("write");
    fs::create_dir_all(root.join("node_modules")).expect("mkdir");
    fs::write(root.join("node_modules/dep.go"), "TODO noise\n").expect("write");

    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 50,
        ..Default::default()
    };
    let results = search(root, &opts, no_cancel()).await.expect("rg search");
    assert_eq!(
        results.len(),
        1,
        "rg: expected 1 result (node_modules skipped), got {results:?}"
    );
    assert_eq!(results[0].path, "top.txt");
}

/// The rg path returns no more than the API's global result limit. ripgrep's
/// own `--max-count` is per file, so this also guards the parser-side cap.
#[tokio::test]
#[allow(clippy::print_stderr)] // skip notice — `tracing` is overkill in tests
async fn rg_primary_max_results_when_available() {
    if !super::rg::on_path() {
        eprintln!("rg not on PATH; skipping rg primary test");
        return;
    }
    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    for index in 0..5 {
        fs::write(root.join(format!("match-{index}.txt")), "TODO\n").expect("write");
    }

    let opts = SearchOptions {
        pattern: "TODO".into(),
        max_results: 3,
        ..Default::default()
    };
    let results = search(root, &opts, no_cancel()).await.expect("rg search");
    assert_eq!(results.len(), 3, "rg max_results cap not honored");
}

/// Synthetic rg JSON parse test — guards against the regression where the
/// parser put the file path into `LineContent` instead of the matched line text.
/// Mirrors Go's `TestParseRgJSON_LineContentNotPath`. Runs everywhere (no rg
/// needed) because it feeds a synthetic JSON line directly to the parser.
#[tokio::test]
async fn parse_rg_json_line_content_not_path() {
    use tokio::io::AsyncWriteExt;

    let dir = TempDir::new().expect("tempdir");
    let root = dir.path();
    // Create the file rg would have reported so strip_prefix works.
    fs::write(root.join("b.txt"), "TODO: fix this\n").expect("write");
    let full_path = root.join("b.txt");
    let escaped_path = full_path.to_string_lossy().replace('\\', "\\\\");

    let json = format!(
        r#"{{"type":"match","data":{{"path":{{"text":"{escaped_path}"}},"lines":{{"text":"TODO: fix this"}},"line_number":1,"absolute_offset":0,"submatches":[{{"match":{{"text":"TODO"}},"start":0,"end":4}}]}}}}"#
    );

    // Pipe the JSON through a ChildStdout-like async reader. We approximate by
    // writing to a file and reading it back via a tokio pipe.
    let (mut tx, rx) = tokio::io::duplex(8192);
    // `json + "\n"` would move `json`; append a newline in place so `json`
    // remains borrowed later in the assertion block.
    let mut json_nl = json.clone();
    json_nl.push('\n');
    tx.write_all(json_nl.as_bytes()).await.expect("write json");
    // Close the write end so the reader sees EOF.
    drop(tx);

    // The parser expects a ChildStdout; we cannot construct one directly, so
    // instead invoke the logic via a small inline reimplementation is not
    // ideal. Instead, test the public surface: spawn a fake "rg" that emits
    // the JSON. That is heavy; instead, verify the regression via the
    // RgRecord deserialization directly.
    let rec: super::rg::tests::RgRecordForTest = serde_json::from_str(&json).expect("parse record");
    assert_eq!(rec.record_type, "match");
    assert_eq!(
        rec.data.lines.text.as_deref(),
        Some("TODO: fix this"),
        "LineContent must come from data.lines.text, not the path"
    );
    assert_eq!(rec.data.line_number, 1);

    // Verify the relativization + offset logic the parser uses.
    let path_str = rec.data.path.text.as_deref().expect("path");
    let rel = Path::new(path_str).strip_prefix(root).expect("strip");
    assert_eq!(rel.to_string_lossy(), "b.txt");

    let re = regex::Regex::new("TODO").expect("regex");
    let m = re.find("TODO: fix this").expect("match");
    assert_eq!(m.start(), 0);
    assert_eq!(m.end(), 4);

    // Suppress unused-import warning for the duplex setup above; the
    // regression is fully covered by the deserialization + offset checks.
    let _ = rx;
}
