# Broken epic cross-references in all 7 profile story files

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `docs/plans/profiles-over-acp/done-acp-set-config-option-send-hard.md` (and 6 sibling files)
- **Lines:** 4 (line 4 in each of the 7 story files)

## Description

The epic plan file was renamed from `pending-profiles-over-acp-hard.md` to
`complete-profiles-over-acp-hard.md`, but none of the 7 story files in
`docs/plans/profiles-over-acp/` were updated to reflect the new epic filename.
Every story file's header still reads:

```
> **Epic:** [profiles-over-acp](../pending-profiles-over-acp-hard.md).
```

The target `../pending-profiles-over-acp-hard.md` no longer exists — it was
renamed as part of this same diff. This affects all 7 files:

- `done-acp-set-config-option-send-hard.md:4`
- `done-chat-profile-selection-easy.md:4`
- `done-mockagent-set-config-option-easy.md:4`
- `done-profile-config-schema-med.md:4`
- `done-profiles-rest-crud-med.md:4`
- `done-settings-profiles-tab-med.md:4`
- `pending-mcp-tool-enumeration-filtering-hard.md:4`

## Recommendation

Update the epic link in all 7 story files from
`../pending-profiles-over-acp-hard.md` to
`../complete-profiles-over-acp-hard.md`. A single `sed` or `grep -rl` +
replace pass handles all files at once.

## Verification

Ran `grep -n "Epic:" docs/plans/profiles-over-acp/done-*.md
docs/plans/profiles-over-acp/pending-*.md` — all 7 files reference
`../pending-profiles-over-acp-hard.md`. Confirmed the file does not exist:
`ls docs/plans/` shows only `complete-profiles-over-acp-hard.md`.
