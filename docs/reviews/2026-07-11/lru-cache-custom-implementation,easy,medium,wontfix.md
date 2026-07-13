# Custom LRU Cache Implementation

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `/media/adam/extex/projects/project-hail-larry/internal/files/files.go`
- **Lines:** 209-280

## Description

The `internal/files` package implements a custom Least Recently Used (LRU) cache (`lruCache` and `lruNode` structs) to cache file sync baseline contents.
Hand-rolling custom data structures like LRU caches increases the codebase footprint and increases the likelihood of maintenance overhead. Furthermore, the custom cache is not thread-safe and relies on manual pointer manipulation (e.g., node insertion and removal), which is prone to subtle bugs.

## Recommendation

Replace the custom LRU cache implementation with a well-tested, popular library such as:
- **`github.com/hashicorp/golang-lru/v2`**

This library is the industry standard for LRU caches in Go, is fully thread-safe, and supports Go generics, allowing type-safe key and value declarations.

## Verification

Code inspection of [internal/files/files.go](file:///media/adam/extex/projects/project-hail-larry/internal/files/files.go#L209-L280) reveals a hand-rolled LRU cache struct and associated insertion/lookup logic using a custom doubly-linked list.

## Resolution (2026-07-12) — WONTFIX
The cache is ~70 lines of correct, stdlib-only (`container/list`) code already guarded by `f.mapMu` on every access (thread-safety is a false positive), and replacing it with `hashicorp/golang-lru/v2` would add an external dependency for no real benefit.
