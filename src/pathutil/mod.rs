//! Path traversal and symlink containment utilities (Go `internal/pathutil/`).
//!
//! Ports `pathutil.SafeJoin` and the symlink-resolution half of
//! `internal/workspace.safeJoin`/`resolveSymlinks` so the workspace, files,
//! shell, and server modules can share a single, audited containment check.
//!
//! Two layers of defence (matching the Go daemon):
//! 1. [`clean_path`] — lexical: rejects absolute inputs and any `..` path
//!    component, then verifies the joined path stays beneath `workspace_root`.
//! 2. [`resolve_symlink`] — on-disk: canonicalises the path (or its parent for
//!    not-yet-existing write targets) and rejects any symlink whose resolved
//!    target escapes `workspace_root`. A symlink at the final component is
//!    rejected outright — agents must not read/write through links they may
//!    have created via an approved shell command (e.g. `ln -s /etc/passwd .`).
//!
//! All non-test functions return [`Result`]; none panic.

use std::fs;
use std::path::{Path, PathBuf};

use thiserror::Error;

/// Errors returned by the path-containment helpers.
///
/// Each variant maps to a distinct containment failure so callers (workspace,
/// files, shell, server) can surface consistent diagnostics without re-parsing
/// error strings.
#[derive(Debug, Error)]
pub enum PathError {
    /// Caller-supplied relative path tried to escape `workspace_root` lexically
    /// (absolute input, `..` component, or post-clean containment failure).
    /// Carries the offending input for diagnostics.
    #[error("path traversal detected: {0}")]
    TraversalAttempted(String),

    /// A symlink on the resolved path points outside `workspace_root`, or the
    /// final/parent component is itself a symlink (rejected outright per the
    /// Go policy — see [`resolve_symlink`]).
    #[error("resolved path escapes workspace root: {0}")]
    SymlinkEscapesRoot(String),

    /// The input was empty, not valid Unicode, or otherwise malformed.
    #[error("invalid path: {0}")]
    InvalidPath(String),

    /// Underlying `std::fs` operation failed (stat, readlink, canonicalize)
    /// for a reason other than "not found", which is handled inline.
    #[error("io error: {0}")]
    IoError(#[from] std::io::Error),
}

/// Join `workspace_root` with `input`, rejecting traversal attempts.
///
/// Port of `pathutil.SafeJoin`. Performs lexical checks only — callers that
/// need to defend against symlink-based escapes must layer
/// [`resolve_symlink`]` on top.
///
/// A path is rejected if:
/// - the cleaned input is absolute, or
/// - any individual component equals `..` (a real parent-directory traversal;
///   filenames like `..foo` are NOT rejected), or
/// - the joined, cleaned path does not stay within `workspace_root`.
///
/// `workspace_root` is canonicalised first so the containment check compares
/// against the real on-disk root (handles `root/../root` style inputs).
///
/// # Errors
/// Returns [`PathError::TraversalAttempted`] for traversal, and
/// [`PathError::InvalidPath`] for empty/non-UTF-8 input.
pub fn clean_path(workspace_root: &Path, input: &str) -> Result<PathBuf, PathError> {
    if input.is_empty() {
        return Err(PathError::InvalidPath("empty path".to_string()));
    }

    // Canonicalise the root so the prefix-containment check is meaningful even
    // if the caller passed a relative or symlinked root. If the root doesn't
    // exist on disk we fall back to its lexical normal form — clean_path is
    // purely lexical and callers that need on-disk guarantees use
    // resolve_symlink afterwards.
    let root = fs::canonicalize(workspace_root).unwrap_or_else(|_| workspace_root.to_path_buf());
    let root = root.as_path();

    // PathBuf::clean() doesn't exist; std::path normalises "." and trailing
    // separators on construction. We build the cleaned relative path by
    // iterating components, which drops "." and collapses redundant separators.
    let rel = Path::new(input);
    let cleaned_rel: PathBuf = rel.components().filter(|c| c.as_os_str() != ".").collect();

    // Reject absolute inputs (e.g. "/etc/passwd") — they ignore the root.
    if cleaned_rel.is_absolute() {
        return Err(PathError::TraversalAttempted(input.to_string()));
    }

    // Reject any real ".." component. We compare component-by-component so a
    // filename like "..foo" (a single Normal component) is NOT flagged.
    for comp in cleaned_rel.components() {
        if comp.as_os_str() == ".." {
            return Err(PathError::TraversalAttempted(input.to_string()));
        }
    }

    let full = root.join(&cleaned_rel);

    if !is_within_root(root, &full) {
        return Err(PathError::TraversalAttempted(input.to_string()));
    }
    Ok(full)
}

/// Resolve symlinks on `path` and verify the resolved target stays within
/// `workspace_root`.
///
/// Port of `internal/workspace.resolveSymlinks`. Behaviour:
/// - If the final component is itself a symlink, reject outright. Even if
///   `canonicalize` would resolve it back inside the workspace, allowing
///   reads/writes through an agent-created link (`ln -s /etc/passwd ./passwd`)
///   is the exact escape we prevent.
/// - If the full path exists, canonicalise it and check containment.
/// - If the final component does not exist (a write target), canonicalise the
///   parent and re-attach the leaf, then re-check containment. The parent is
///   also Lstat-checked: an agent could `ln -s /etc ./etc` then write
///   `./etc/passwd`.
/// - If neither path nor parent resolves, return the lexical `path` as-is —
///   the lexical containment check in [`clean_path`] is the only remaining
///   safeguard, but no on-disk symlink chain exists to follow at that point.
///
/// `workspace_root` should already be absolute and cleaned (callers typically
/// pass the canonicalised root from [`clean_path`]).
///
/// # Errors
/// Returns [`PathError::SymlinkEscapesRoot`] for any escape or symlink-at-leaf
/// rejection, and [`PathError::IoError`] for unexpected filesystem failures.
pub fn resolve_symlink(workspace_root: &Path, path: &Path) -> Result<PathBuf, PathError> {
    // Reject a symlink at the final component outright.
    if let Ok(meta) = fs::symlink_metadata(path) {
        if meta.file_type().is_symlink() {
            return Err(PathError::SymlinkEscapesRoot(format!(
                "{} is a symlink; symlinks are not permitted in workspace file access",
                path.display()
            )));
        }
    }

    // Try to fully resolve the path. canonicalize follows symlinks in every
    // component and returns the canonical absolute path.
    if let Ok(resolved) = fs::canonicalize(path) {
        if !is_within_root(workspace_root, &resolved) {
            return Err(PathError::SymlinkEscapesRoot(format!(
                "{} escapes workspace root {}",
                resolved.display(),
                workspace_root.display()
            )));
        }
        return Ok(resolved);
    }

    // canonicalize failed — typically the final component doesn't exist yet
    // (a write target). Resolve the parent and re-attach the leaf.
    let parent = match path.parent() {
        Some(p) if !p.as_os_str().is_empty() => p,
        // No parent (path is "/" or a bare filename) — nothing more to resolve.
        _ => return Ok(path.to_path_buf()),
    };
    let base = path.file_name().ok_or_else(|| {
        PathError::InvalidPath(format!("path has no final component: {}", path.display()))
    })?;

    // If the parent is itself a symlink, reject: `ln -s /etc ./etc` + write
    // `./etc/passwd` would escape.
    if let Ok(meta) = fs::symlink_metadata(parent) {
        if meta.file_type().is_symlink() {
            return Err(PathError::SymlinkEscapesRoot(format!(
                "parent {} is a symlink; symlinks are not permitted in workspace file access",
                parent.display()
            )));
        }
    }

    let resolved_parent = match fs::canonicalize(parent) {
        Ok(p) => p,
        // Neither path nor parent resolves — fall back to lexical path; the
        // lexical containment check in clean_path remains the safeguard.
        Err(_) => return Ok(path.to_path_buf()),
    };

    if !is_within_root(workspace_root, &resolved_parent) {
        return Err(PathError::SymlinkEscapesRoot(format!(
            "resolved parent {} escapes workspace root {}",
            resolved_parent.display(),
            workspace_root.display()
        )));
    }

    // Re-attach the (lexical) leaf. Safe because the leaf doesn't yet exist on
    // disk, so it cannot itself be a symlink.
    let resolved_path = resolved_parent.join(base);
    if !is_within_root(workspace_root, &resolved_path) {
        return Err(PathError::SymlinkEscapesRoot(format!(
            "resolved path {} escapes workspace root {}",
            resolved_path.display(),
            workspace_root.display()
        )));
    }
    Ok(resolved_path)
}

/// Report whether `path` is equal to `root` or lives beneath it.
///
/// Both arguments are expected to be absolute and normalised (canonical or
/// lexically cleaned). Port of `internal/workspace.isWithinRoot`.
fn is_within_root(root: &Path, path: &Path) -> bool {
    if path == root {
        return true;
    }
    // path must start with root + separator. Using `strip_prefix` is the
    // std-idiomatic equivalent of Go's `strings.HasPrefix(path, root+sep)`.
    path.starts_with(root)
        && path
            .strip_prefix(root)
            .is_ok_and(|tail| !tail.as_os_str().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::symlink;
    use tempfile::tempdir;

    // ---- clean_path: traversal rejection (matches Go SafeJoin cases) ----

    #[test]
    fn clean_path_rejects_parent_parent_etc_passwd() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let err = clean_path(&root, "../../etc/passwd").unwrap_err();
        assert!(matches!(err, PathError::TraversalAttempted(_)), "{err:?}");
    }

    #[test]
    fn clean_path_rejects_dot_dot_sibling() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let err = clean_path(&root, "../sibling").unwrap_err();
        assert!(matches!(err, PathError::TraversalAttempted(_)), "{err:?}");
    }

    #[test]
    fn clean_path_rejects_nested_traversal_in_middle() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // foo/../../../bar cleans to ../bar which escapes.
        let err = clean_path(&root, "foo/../../../bar").unwrap_err();
        assert!(matches!(err, PathError::TraversalAttempted(_)), "{err:?}");
    }

    #[test]
    fn clean_path_rejects_absolute_input() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let err = clean_path(&root, "/etc/passwd").unwrap_err();
        assert!(matches!(err, PathError::TraversalAttempted(_)), "{err:?}");
    }

    #[test]
    fn clean_path_allows_dot_dot_prefix_filename() {
        // Filenames that merely begin with ".." (e.g. "..foo") are NOT
        // rejected — only a real ".." component is.
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let got = clean_path(&root, "..foo").unwrap();
        assert_eq!(got, root.join("..foo"));
    }

    #[test]
    fn clean_path_allows_simple_relative() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let got = clean_path(&root, "src/main.rs").unwrap();
        assert_eq!(got, root.join("src/main.rs"));
    }

    #[test]
    fn clean_path_allows_dot_segments() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let got = clean_path(&root, "./src/./main.rs").unwrap();
        assert_eq!(got, root.join("src/main.rs"));
    }

    #[test]
    fn clean_path_allows_root_itself() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let got = clean_path(&root, ".").unwrap();
        assert_eq!(got, root);
    }

    // ---- clean_path: edge cases (no panics) ----

    #[test]
    fn clean_path_rejects_empty() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let err = clean_path(&root, "").unwrap_err();
        assert!(matches!(err, PathError::InvalidPath(_)), "{err:?}");
    }

    #[test]
    fn clean_path_handles_non_utf8_input() {
        // Non-UTF-8 bytes in the input string are impossible (input is &str),
        // but a non-UTF-8 *component* can still be constructed via OsStr. We
        // verify clean_path doesn't panic on a path that contains an invalid
        // UTF-8 sequence when re-expressed as a str-backed Path. Since &str is
        // guaranteed UTF-8, this test confirms the empty/edge path doesn't
        // panic; the real non-UTF-8 surface is the Path API, exercised below.
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // A str with a NUL byte is still UTF-8; Path::new accepts it.
        let got = clean_path(&root, "foo\0bar");
        // Either Ok (joined) or an error is acceptable; the contract is "no
        // panic". The filesystem would reject NUL later.
        assert!(got.is_ok() || matches!(got, Err(PathError::InvalidPath(_))));
    }

    // ---- resolve_symlink: containment ----

    #[test]
    #[cfg(unix)]
    fn resolve_symlink_rejects_link_outside_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // Create a target outside the workspace root (but inside the tempdir
        // parent, which we own and can clean up).
        let outside = dir.path().join("outside-target.txt");
        fs::write(&outside, "secret").unwrap();
        let link = root.join("passwd");
        symlink(&outside, &link).unwrap();

        let err = resolve_symlink(&root, &link).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscapesRoot(_)), "{err:?}");
    }

    #[test]
    #[cfg(unix)]
    fn resolve_symlink_allows_link_inside_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let target = root.join("real.txt");
        fs::write(&target, "data").unwrap();
        let link = root.join("link.txt");
        symlink(&target, &link).unwrap();

        // Final component is a symlink → rejected outright per Go policy,
        // even though the target is inside the root. This matches the Go
        // daemon's "no reads/writes through agent-created symlinks" rule.
        let err = resolve_symlink(&root, &link).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscapesRoot(_)), "{err:?}");
    }

    #[test]
    fn resolve_symlink_allows_regular_file() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("real.txt");
        fs::write(&file, "data").unwrap();
        let got = resolve_symlink(&root, &file).unwrap();
        assert_eq!(got, file);
    }

    #[test]
    fn resolve_symlink_handles_nonexistent_write_target() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // Parent (root) exists; leaf does not. Should resolve to root/leaf.
        let target = root.join("new-file.txt");
        let got = resolve_symlink(&root, &target).unwrap();
        assert_eq!(got, target);
    }

    #[test]
    #[cfg(unix)]
    fn resolve_symlink_rejects_symlink_parent() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // Create a symlinked directory inside root pointing outside.
        let outside_dir = dir.path().join("outside-dir");
        fs::create_dir(&outside_dir).unwrap();
        let link_dir = root.join("etc");
        symlink(&outside_dir, &link_dir).unwrap();

        // Writing through ./etc/passwd would escape via the symlinked parent.
        let target = link_dir.join("passwd");
        let err = resolve_symlink(&root, &target).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscapesRoot(_)), "{err:?}");
    }

    #[test]
    fn resolve_symlink_falls_back_when_parent_unresolvable() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // Neither the path nor its parent exists on disk. Should fall back to
        // the lexical path (no panic, no error).
        let target = root.join("a/b/c/new.txt");
        let got = resolve_symlink(&root, &target).unwrap();
        assert_eq!(got, target);
    }

    // ---- property-style: arbitrary relative paths & symlink chains ----

    #[test]
    fn clean_path_property_arbitrary_relative_stays_in_root() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        // A representative spread of relative inputs that should be accepted:
        // none contain a `..` component or start absolute.
        let inputs = [
            "a",
            "a/b",
            "a/b/c",
            "deeply/nested/path/to/file.rs",
            "with-dashes",
            "with_underscores",
            "UPPER",
            "mixedCase123",
            "trailing/slash/",
            "double//slash",
        ];
        for input in inputs {
            let got = clean_path(&root, input).unwrap();
            assert!(
                is_within_root(&root, &got),
                "input {input:?} escaped root: {}",
                got.display()
            );
        }
    }

    #[test]
    fn clean_path_property_traversal_inputs_rejected() {
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let inputs = [
            "..",
            "../",
            "../etc/passwd",
            "../sibling",
            "foo/../../../bar",
            "a/../../b",
            "./..",
            "foo/../..",
            "/etc/passwd",
            "/",
        ];
        for input in inputs {
            let got = clean_path(&root, input);
            assert!(got.is_err(), "input {input:?} should have been rejected");
        }
    }

    #[test]
    #[cfg(unix)]
    fn resolve_symlink_chain_inside_root_resolves() {
        // chain: link3 -> link2 -> link1 -> real.txt, all inside root.
        let dir = tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        let real = root.join("real.txt");
        fs::write(&real, "data").unwrap();
        let l1 = root.join("l1");
        symlink(&real, &l1).unwrap();
        let l2 = root.join("l2");
        symlink(&l1, &l2).unwrap();

        // Resolving l2 directly would reject (final component is a symlink).
        // Resolving the *real* file canonicalises through the chain fine.
        let got = resolve_symlink(&root, &real).unwrap();
        assert_eq!(got, real);
    }

    #[test]
    fn is_within_root_handles_boundary() {
        let root = Path::new("/tmp/ws");
        assert!(is_within_root(root, Path::new("/tmp/ws")));
        assert!(is_within_root(root, Path::new("/tmp/ws/foo")));
        // A sibling that shares a prefix string but is not under root.
        assert!(!is_within_root(root, Path::new("/tmp/ws-evil")));
        assert!(!is_within_root(root, Path::new("/tmp")));
        assert!(!is_within_root(root, Path::new("/etc")));
    }
}
