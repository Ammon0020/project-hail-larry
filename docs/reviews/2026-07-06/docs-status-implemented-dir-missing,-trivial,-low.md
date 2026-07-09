# STATUS.md references non-existent implemented/ directory

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `docs/STATUS.md`
- **Lines:** 52

## Description

Line 52 states `Done earlier (moved to \`implemented/\`): go-core-config-data-race, ...`. There is no `implemented/` directory anywhere in the repo (not under `docs/plans/`, not at root). A reader trying to find the original writeups for those four resolved findings cannot locate them. This is a dangling cross-reference left over from a refactor that moved/removed the directory without updating the citation.

## Recommendation

Either create `docs/plans/implemented/` and move the original finding docs there, or rewrite the line to drop the path citation (e.g. `Done earlier (resolved in prior passes): ...`) since the finding IDs are self-descriptive.

## Verification

`find_file_by_name` for `docs/plans/implemented/**` returned no files. `grep` for `implemented/` across the whole repo found exactly one match — this line — confirming no other file references or defines the directory.
