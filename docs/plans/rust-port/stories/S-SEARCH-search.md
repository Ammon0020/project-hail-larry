# Story S-SEARCH: Workspace Content Search

> **Phase:** 2 | **Depends on:** — | **Go source:** `internal/search/` (437 lines)

## Summary

Port workspace-wide content search: regex/substring matching, file
filtering (ignore `node_modules`, `.git`, etc.), result line context.

## Go Source

`internal/search/search.go` — `Options` struct, `Result` struct, walks
file tree, applies ignore patterns, reads files, matches regex, returns
matching lines with line numbers and context.

## Rust Implementation

- Regex: `regex` crate
- File walking: `walkdir` crate (or `ignore` crate which handles
  `.gitignore` natively — consider for better ignore matching)
- Ignore patterns: port the ignore list (`node_modules`, `.git`, `.next`,
  `dist`, `build`, `out`, `coverage`, `vendor`)
- Read files with `std::fs::read_to_string`, skip binary files
- Port `search_test.go`

## Acceptance Criteria

- [ ] Regex and substring search work
- [ ] Ignore patterns skip configured directories
- [ ] Results include file path, line number, matching line, context
- [ ] Binary files skipped without errors
- [ ] `cargo test search` passes
