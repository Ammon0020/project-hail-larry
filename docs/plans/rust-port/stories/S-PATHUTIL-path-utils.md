# Story S-PATHUTIL: Path Traversal & Symlink Utils

> **Phase:** 1 | **Depends on:** — | **Go source:** `internal/pathutil/` (44 lines)

## Summary

Port path validation utilities: traversal prevention, symlink containment,
path cleaning within workspace bounds.

## Go Source

`internal/pathutil/pathutil.go` — `CleanPath`, `ResolveSymlink`, traversal
checks used by workspace, files, shell, and server packages.

## Rust Implementation

- Module: `pathutil` (or `src/pathutil.rs`)
- Use `std::path::{Path, PathBuf}`, `std::fs::canonicalize`
- Symlink containment: walk path components, reject if resolved path
  escapes workspace root
- Port all tests from `pathutil_test.go` (if exists — check)

## Acceptance Criteria

- [ ] `cargo test pathutil` passes
- [ ] Path traversal attempts rejected (e.g. `../../etc/passwd`)
- [ ] Symlinks pointing outside workspace root rejected
- [ ] No panics on edge cases (empty path, root path, non-UTF8)
