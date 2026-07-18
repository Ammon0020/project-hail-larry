# User-noted improvements

Users write the key missing features they see.

**Difficulty:** `Trivial` < `Small` < `Medium` < `Large`
**Urgency:** `High` > `Medium` > `Low`

Items are grouped by code area so a developer can stay in one context while working through a batch. Within each group, quick wins come first. Dependencies are noted inline.

---

## Git

- [ ] **[Large/Medium]** Add a git action bar item and diff viewer and a way to stage/commit/push if a git repo is detected.
  - *Backend only has `runGit()` for agent context (`internal/acp/context.go:407-425`); no git API endpoints in `internal/server/`. Frontend hardcodes "main" in `EditorPane.tsx:496`. Needs: status/diff/stage/commit/push endpoints, a diff viewer (`@codemirror/merge` not installed), and a git action bar UI. Foundational for the items below.*
- [ ] **[Small/Medium]** Add a way to initialize git in the workspace if it isn't detected.
  - *Depends on the git detection infrastructure above. Add `POST /api/workspaces/{id}/git/init` + a UI trigger (WorkspaceHeader/settings).*

---

## Chat

- [ ] **[Large/Medium]** Add a little popup/slideup above the chat input with icons for edited files and subagents and anything else we choose to add. Popup only shows if something is in one of them. The one that has a status has a blue dot indicator on its top right. Extremely compact. Eg. If a file is changed, it shows the blue dot by that and when you click it, the popup slides open more to reveal the list of edited files and changed line counts (plus and minus) on the right of the filename, and to the right of the edits is an accept edits button and a revert button. Revert requires a confirmation. When you click/tap any file it opens the agent diff in an editor tab.
  - *Popup pattern exists (`web/src/components/chat/McpPopout.tsx`); chat input is `ChatComposer.tsx`. Missing: edited-files list with +/- line counts, subagent status tracking, blue-dot indicator, accept/revert actions, and an agent diff viewer. Depends on the git diff viewer above for diff rendering.*

---

## UI
- [ ] If a tab in the editor isn't marked to keep open, it closes on page reload. 

## Agent Chat
- [ ] Fix Devin auto-detect models
- [ ] Fix Cursor Agent auto-detect models