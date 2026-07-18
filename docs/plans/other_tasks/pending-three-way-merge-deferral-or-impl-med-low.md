# Task: Server-side three-way merge (or formal frontend deferral)

> **Status:** pending | **Difficulty:** med | **Urgency:** low
> **Origin:** S-FILES audit (2026-07-18). Epic: rust-port.

## Problem

`docs/plans/rust-port/active-S-FILES-file-sync-merge-med.md` has one unchecked
AC: "Three-way merge produces correct results for all test cases." The Rust
port deliberately matches Go Phase 1 behavior: the server returns
`StaleRevision` and the frontend uses `@codemirror/merge` for the actual merge.
A base-content cache (`get_base_content`) supports the frontend merge.

This is an intentional design choice, not a bug — but the AC remains unchecked
because no server-side merge logic exists.

## Scope

**Option A (preferred): formally defer**
- Update the story AC to reflect that three-way merge is frontend-owned
- Document the architecture decision (server returns StaleRevision + base
  content; frontend performs the merge)
- Check off the AC with a note pointing to the deferral

**Option B (if server-side merge is needed later):**
- Add `similar`/`diffy` dependency
- Implement server-side three-way merge in `src/files/merge.rs`
- Add tests for all Go merge test cases
- Wire into `save_inner` when revision is stale

## Acceptance criteria

- [ ] Either: AC checked off with formal deferral note, OR server-side merge
      implemented with tests

## Out of scope

- Changing the frontend merge approach (already working with @codemirror/merge)
