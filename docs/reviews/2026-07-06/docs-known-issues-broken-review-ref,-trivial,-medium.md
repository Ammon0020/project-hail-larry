# known-issues.md references non-existent review directory review/2026-06-27/

- **Difficulty:** trivial
- **Urgency:** medium
- **File:** `docs/known-issues.md`
- **Lines:** 6

## Description

Line 6 reads `## Web frontend — deferred review findings (from \`review/2026-06-27/\`)`. This directory does not exist in the repo. The only review snapshot present is `docs/reviews/2026-07-06/` (note: plural `reviews`, different date). Additionally the path uses singular `review/` while the actual convention (per AGENTS.md line 94 and the filesystem) is plural `reviews/`. This is a broken cross-reference: a reader following it to find the original findings hits a dead end, and the date mismatch obscures when the review actually happened.

## Recommendation

Either restore/move the original 2026-06-27 review snapshot under `docs/reviews/2026-06-27/`, or update the reference to point to where the findings actually live now (e.g. `docs/reviews/2026-07-06/` if consolidated there, or remove the parenthetical source citation since the findings are now fully resolved and summarized inline). At minimum fix `review/` → `reviews/` for consistency with the documented convention.

## Verification

Ran `find docs -type d`; only `docs/reviews/2026-07-06` exists. `find_file_by_name` for `docs/reviews/2026-06-27/**` and `docs/review/**` both returned no files. AGENTS.md line 94 documents the convention as `docs/reviews/<date>/`.
