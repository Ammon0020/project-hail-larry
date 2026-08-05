# Chat edited-files popup with accept/revert actions

> **Status:** pending | **Difficulty:** large | **Urgency:** medium
> **Source:** user-noted improvements — Chat section

## Goal

Add a compact popup/slideup above the chat input showing icons for edited
files, subagent status, and other activity. A blue dot indicator appears on
items with a status. Clicking expands the popup to reveal details.

## Behavior

- Popup only shows when something is active (edited files, subagents, etc.)
- Blue dot indicator on items with a status
- **Edited files section**: list of files with +/- line counts on the right
  of the filename, plus accept-edits and revert buttons. Revert requires
  confirmation. Clicking a file opens the agent diff in an editor tab.
- **Subagent section**: status of subagent tasks
- Extremely compact when collapsed; expands on click/tap
- Popup pattern exists (`web/src/components/chat/McpPopout.tsx`); chat input
  is `ChatComposer.tsx`

## Dependencies

- Uses the git diff viewer (`GitDiffViewer.tsx`) for diff rendering
- Needs backend support for tracking edited files per session with line counts

## Acceptance

- [ ] Popup appears above chat input when files are edited or subagents active
- [ ] Blue dot indicator on items with status
- [ ] Edited files list with +/- line counts
- [ ] Accept edits and revert buttons (revert requires confirmation)
- [ ] Clicking a file opens the agent diff in an editor tab
- [ ] Mobile-friendly (tap to expand/collapse)
- [ ] `make check` passes
