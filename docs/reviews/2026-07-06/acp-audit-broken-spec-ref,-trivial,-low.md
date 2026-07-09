# Broken/ambiguous relative reference to spec.md in audit intro

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `docs/reviews/2026-07-06/acp-audit.md`
- **Lines:** 3

## Description

Line 3 says "compares the current ACP client implementation ... against the official ACP specification (see `spec.md`)." There is no `spec.md` in the audit's directory (`docs/reviews/2026-07-06/`); the authoritative spec lives at `docs/reference/acp/spec.md`. The same file's footer (line 82) correctly uses the full path, so line 3 is an inconsistent, ambiguous cross-reference that a reader cannot resolve without searching.

## Recommendation

Change "`spec.md`" on line 3 to "`docs/reference/acp/spec.md`" to match the footer reference and AGENTS.md's documented path convention.

## Verification

Confirmed `docs/reviews/2026-07-06/` contains only `acp-audit.md` (no `spec.md`), and that `docs/reference/acp/spec.md` exists. Confirmed line 82 of the same file uses the correct full path.
