# Chat edited-files popup — Story 3: agent diff viewer tab

> **Status:** pending | **Difficulty:** small | **Urgency:** medium
> **Source:** user-noted improvements — Chat section
> **Parent:** `pending-user_noted_improvements-large-high.md`
> **Depends on:** `pending-chat-edited-files-popup-2-popup-ui-med-med.md`

## Goal

When the user clicks/taps a file in the edited-files popup, open an editor tab
showing the agent's diff for that file — the changes the agent made, not the git
diff.

## Scope

- **Diff source**: Compare the file's current content against its pre-edit
  revision (from the revision tracking system). The backend already tracks
  revisions via `FileRevisionUpdated` events.
- **Viewer**: Reuse the CodeMirror merge diff viewer from `GitDiffViewer.tsx`,
  adapted for agent-edit context (no git staging/commit chrome).
- **Tab lifecycle**: Opens as a preview tab; closes when the user accepts or
  reverts the edits for that file.

## Dependencies

- Story 2's popup UI.
- Existing revision tracking (`FileRevisionUpdated` events, `src/files/`).
- `GitDiffViewer.tsx` merge viewer component.

## Acceptance

- Clicking a file in the edited-files popup opens a diff tab.
- Diff shows the agent's changes (current vs. pre-edit revision).
- Tab closes on accept/revert.
- `make check` passes.

## Verification

```text
make check
```
