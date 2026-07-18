# Story S-SEARCH: Workspace Content Search

> **Phase:** 2 | **Depends on:** S-INTERFACES | **Go source:** `internal/search/` (437 lines)

## Summary

Port workspace-wide content search: regex/substring matching, file
filtering (ignore `node_modules`, `.git`, etc.), result line context.

## Go Source

`internal/search/search.go` — `Options` struct, `Result` struct, walks
file tree, applies ignore patterns, reads files, matches regex, returns
matching lines with line numbers and context.

## Rust Implementation

Prefer the same hybrid strategy as Go (`internal/search`): shell out to
`rg` when available, fall back to a native walker. Do not reimplement a
full pure-Rust ripgrep first.

1. **Primary path (matches Go):** spawn `rg --json` via `tokio::process::Command`
   when `rg` is on `PATH`. Parse JSON events for path/line/offsets; honor
   `IgnoreCase`, `MaxResults`, `FilePattern`, `ContextLines` exactly as Go does.
2. **Fallback:** `ignore` crate walk + `regex` crate when `rg` is missing.
   Configure `ignore` to honor the **explicit** ignore list only (same set as
   Go / workspace tree) before opting into `.gitignore` semantics.
3. Ignore dirs (must match Go + workspace tree): `.git`, `node_modules`,
   `vendor`, `dist`, `build`, `.next`, `target`, `.cache`, `coverage`, `out`.
4. Skip binary files in the fallback without erroring.
5. Port `search_test.go` for both code paths when practical (or mock `rg`).

## Acceptance Criteria

- [x] Regex and substring search work
- [x] Prefer `rg --json` when present; native fallback when not
- [x] Ignore patterns skip configured directories
- [x] Results include file path, line number, matching line, context
- [x] Binary files skipped without errors
- [x] `cargo test search` passes
