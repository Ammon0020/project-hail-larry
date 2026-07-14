# Story S-WORKSPACE: Workspace Manager

> **Phase:** 3 | **Depends on:** S-FILES, S-SEARCH | **Go source:** `internal/workspace/` (750 lines)

## Summary

Port workspace registration, file tree generation, file read/write (with
revision tracking via S-FILES), raw file serving, search (via S-SEARCH),
git info.

## Go Source

`internal/workspace/` — `Manager`, `Register`, `List`, `Remove`,
`FileTree` (recursive directory walk, ignore patterns), `ReadFile` (text
content + revision + binary/previewable flags), `WriteFile` (optimistic
revision via S-FILES), `FilePath` (validated absolute path for raw
serving), `Search` (delegates to S-SEARCH).

## Rust Implementation

- File tree: `walkdir` crate, apply ignore patterns
- Binary detection: `infer` crate (magic bytes) or check for null bytes
- Previewable flag: port the extension list (SVG, OBJ, CSV, etc.)
- Git info: `std::process::Command` or `gix` crate (consider lightweight
  git branch detection without full git lib)
- Implements `WorkspaceManager` trait from S-INTERFACES
- Port tests

## Acceptance Criteria

- [ ] Register/List/Remove workspaces
- [ ] FileTree generates correct recursive tree with ignore patterns
- [ ] ReadFile returns content, revision, binary flag, previewable flag
- [ ] WriteFile uses optimistic revision (delegates to S-FILES)
- [ ] FilePath validates path traversal + symlinks
- [ ] Search delegates to S-SEARCH
- [ ] `cargo test workspace` passes
