# Task: Server-side three-way merge (or formal frontend deferral)

> **Status:** complete | **Difficulty:** med | **Urgency:** low
> **Origin:** S-FILES audit (2026-07-18). Epic: rust-port.
> **Decision (2026-07-18):** Option A — formally defer. Merge is frontend-owned.

## Problem

S-FILES had an unchecked AC for server-side three-way merge. The Rust port
matches Go Phase 1: server returns `StaleRevision`; UI merges with
`@codemirror/merge`. Base-content cache supports that path.

## Decision

**Option A (chosen):** three-way merge is frontend-owned. Server returns
`StaleRevision` + base content. No `src/files/merge.rs` unless a later epic
requires server-side merge.

Story closed as `complete-S-FILES-file-sync-merge-med.md` with the AC noted
as deferred-to-frontend (not a missing port).

## Acceptance criteria

- [x] AC checked off with formal deferral note (Option A)
