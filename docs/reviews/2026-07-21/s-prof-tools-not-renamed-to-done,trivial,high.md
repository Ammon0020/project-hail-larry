# S-PROF-TOOLS story file not renamed to done- despite being marked complete

- **Difficulty:** trivial
- **Urgency:** high
- **File:** `docs/plans/profiles-over-acp/pending-mcp-tool-enumeration-filtering-hard.md`
- **Lines:** 1-5

## Description

The epic plan `docs/plans/complete-profiles-over-acp-hard.md` (line 62) marks
S-PROF-TOOLS as `✅ done` and links to
`profiles-over-acp/done-mcp-tool-enumeration-filtering-hard.md`. STATUS.md
(line 40) claims "all 7 stories done." However, the actual story file is still
named `pending-mcp-tool-enumeration-filtering-hard.md` — it was never renamed
to `done-`. Its internal status (line 2) still reads `> **Status:** pending`.
This creates three problems:

1. **Broken link in the epic plan:** the epic references
   `done-mcp-tool-enumeration-filtering-hard.md` which does not exist.
2. **Contradictory status:** the file says "pending" while the epic and
   STATUS.md say "done."
3. **Convention violation:** AGENTS.md requires status-prefixed filenames
   ("rename them when status changes"). All 6 sibling stories were renamed
   but this one was missed.

The implementation IS complete — `apply_tool_whitelist` exists in
`src/mcp/tools.rs:235` and the test
`load_session_mcp_servers_respects_profile_tool_whitelist` exists in
`src/acp/core.rs:3472`. Only the docs were not updated.

## Recommendation

1. Rename `pending-mcp-tool-enumeration-filtering-hard.md` →
   `done-mcp-tool-enumeration-filtering-hard.md`.
2. Update line 2: `> **Status:** pending` → `> **Status:** done`.
3. Update line 4: fix the epic link to
   `../complete-profiles-over-acp-hard.md` (see the broken-cross-reference
   finding).

## Verification

- `ls docs/plans/profiles-over-acp/` shows 6 `done-*` files and 1
  `pending-mcp-tool-enumeration-filtering-hard.md`.
- `head -5` of that file confirms `Status: pending` and a link to
  `../pending-profiles-over-acp-hard.md`.
- `grep -n "S-PROF-TOOLS" docs/plans/complete-profiles-over-acp-hard.md`
  shows `✅ done` with a link to `done-mcp-tool-enumeration-filtering-hard.md`
  (non-existent target).
- `grep -rn "apply_tool_whitelist" src/mcp/tools.rs` confirms the code
  exists (line 235), proving the story was implemented but the file was
  not renamed.
