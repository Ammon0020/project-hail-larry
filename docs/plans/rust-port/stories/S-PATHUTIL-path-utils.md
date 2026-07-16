# Story S-PATHUTIL: Path Traversal & Symlink Utils

> **Phase:** 1 | **Depends on:** — | **Go source:** `internal/pathutil/` (44 lines)

## Summary

Port path validation utilities: traversal prevention, symlink containment,
path cleaning within workspace bounds.

## Go Source

`internal/pathutil/pathutil.go` — `CleanPath`, `ResolveSymlink`, traversal
checks used by workspace, files, shell, and server packages.

## Rust Implementation

- Module: `pathutil`.
- Use `std::path::{Path, PathBuf}`, `std::fs::canonicalize`
- Symlink containment: walk path components, reject if resolved path
  escapes workspace root
- **Keep custom** — do not replace with `path-clean` / `path-absolutize` /
  `strict-path`. Policy is stronger than generic crates: reject final-component
  symlink entirely, reject symlinked parent for write targets, lexical `..`
  rejection independent of canonicalize success.
- Port `pathutil_test.go` and add property/fuzz cases for arbitrary relative
  paths, symlink chains, non-UTF8 paths, and TOCTOU-sensitive failures.

## Acceptance Criteria

- [x] `cargo test pathutil` passes (20 tests)
- [x] Path traversal attempts rejected (e.g. `../../etc/passwd`)
- [x] Symlinks pointing outside workspace root rejected
- [x] No panics on edge cases (empty path, root path, non-UTF8)
