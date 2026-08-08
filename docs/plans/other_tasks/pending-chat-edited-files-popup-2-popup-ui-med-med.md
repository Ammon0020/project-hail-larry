# Chat edited-files popup — Story 2: popup UI with accept/revert

> **Status:** pending | **Difficulty:** medium | **Urgency:** medium
> **Source:** user-noted improvements — Chat section
> **Parent:** `pending-user_noted_improvements-large-high.md`
> **Depends on:** `pending-chat-edited-files-popup-1-tracking-med-med.md`

## Goal

Render a compact popup/slideup above the chat input showing icons for edited
files (and subagents, if applicable). The popup only shows when something is in
a category. A blue dot indicator appears on the icon that has a status update.

## Scope

- **Popup component**: Above `ChatComposer.tsx`, following the existing
  `McpPopout.tsx` pattern. Extremely compact when collapsed; slides open to
  reveal the edited-files list on click/tap.
- **Edited-files list**: Filename on the left, changed line counts (+/-) on the
  right, an "accept edits" button and a "revert" button per file (or per batch).
  Revert requires a confirmation.
- **Blue-dot indicator**: On the collapsed icon when there are unread status
  changes.
- **Out of scope**: The agent diff viewer tab (Story 3). Subagent status
  tracking (deferred — no subagent events yet).

## Dependencies

- Story 1's `useEditedFiles` hook and backend edited-files query.
- Git diff viewer (`GitDiffViewer.tsx`) for diff rendering reuse.

## Acceptance

- Popup appears above chat input when the agent has edited files.
- Collapsed state shows a compact icon with blue dot for new changes.
- Expanded state lists edited files with +/- line counts.
- Accept edits and revert (with confirmation) work per file.
- Mobile-friendly: tap to expand/collapse.
- `make check` passes.

## Verification

```text
make check
```
