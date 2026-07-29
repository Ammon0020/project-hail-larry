# User-noted improvements

Users write the key missing features they see.

**Difficulty:** `Trivial` < `Small` < `Medium` < `Large`
**Urgency:** `High` > `Medium` > `Low`

Items are grouped by code area so a developer can stay in one context while working through a batch. Within each group, quick wins come first. Dependencies are noted inline. 

TODO: Break this down into individual stories. 

---

## Git

- [x] **[Large/Medium]** Add a git action bar item and diff viewer and a way to stage/commit/push if a git repo is detected.
  - *Done 2026-07-26 — see `docs/plans/git-action-bar/`. Backend `gix` + git-CLI-porcelain API (`src/git/`, `src/api/git.rs`), CodeMirror merge diff viewer (`web/src/components/git/GitDiffViewer.tsx`), Source Control panel (`GitPanel.tsx`), and dynamic branch in `StatusBar.tsx`.*
- [x] **[Small/Medium]** Add a way to initialize git in the workspace if it isn't detected.
  - *Done 2026-07-26 — `POST /api/workspaces/{id}/git/init` + "Initialize Repository" button in `GitPanel.tsx` (shown when `repoDetected === false`).*

---

## Chat

- [ ] **[Large/Medium]** Add a little popup/slideup above the chat input with icons for edited files and subagents and anything else we choose to add. Popup only shows if something is in one of them. The one that has a status has a blue dot indicator on its top right. Extremely compact. Eg. If a file is changed, it shows the blue dot by that and when you click it, the popup slides open more to reveal the list of edited files and changed line counts (plus and minus) on the right of the filename, and to the right of the edits is an accept edits button and a revert button. Revert requires a confirmation. When you click/tap any file it opens the agent diff in an editor tab.
  - *Popup pattern exists (`web/src/components/chat/McpPopout.tsx`); chat input is `ChatComposer.tsx`. Missing: edited-files list with +/- line counts, subagent status tracking, blue-dot indicator, accept/revert actions, and an agent diff viewer. Depends on the git diff viewer above for diff rendering.*

- [ ] There's a favorite button next to the model, and favorites are marked in the dropdown selector. TODO: Replace that dropdown selector with a menu, including search. Instead of listing models as "gpt xxx high, gpt xxx medium" etc, we give an option to models with multiple thinking levels to select thinking level on said model. Same goes for the fast option for those models that support fast. So at most, a model has: 
  - pin (replace favorite)
  - Thinking (high, medium, low, none, or any other thinking level models have)
  - fast
  - Statistics (token usage, cost, etc.) optional. They'll be added in eventually, so plan ahead for updates. 
  - [ ] Under ACP's model_config options, agents can advertise configurable model traits. We should try to support any of these we can. 

- [ ] Context Window Usage & Cumulative Cost (usage_update): ACP uses a standardized session notification (session/update with sessionUpdate: "usage_update") that allows agents to push real-time context token usage and financial cost metrics back to the client.  
  1. Use a ring next to the send button. As context fills, it fills clockwise until 100% of the given model's context. If compacting, an inside ring should spin around it to indicate that compaction is in progress. 
  2. Hovering shows a popout with:
    - "<x>% (<y>k/<z>k) context used"
    - Cost
    - Below that a timer until the prompt cache expires: "Prompt cache expires in x" down to the second. If prompt cache time can't be seen for the model, it says "Estimated: Prompt cache expires in x". When expired the text turns orange. On mobile, shows on tap for a few seconds or on hold.

---

## UI
- [ ] Move preview, line wrap, and save buttons from the tabs bar at the top of the main canvas to the right side of the line below in the editor, the one where the filepath is shown. This keeps it visible without taking space for tabs, especially on mobile. 
- [ ] x buttons on tabs should have a clearer, slightly larger hitbox when hovered over.
- [ ] When scroll arrows for tab scroll disappear, tabs should not shift. Absolute position, hide and unhide, unless you find a more elegant method.
- [ ] When you switch workspace, it switches which tabs are open. When you reopen, they open back up. Keeps tabs associated with workspace. Tabs are remembered per-workspace on the server. 
  - [ ] Next: Tabs are synced between devices, saved to workspace rather than browser, but only when workspace syncing is enabled. Remove the word "online" from the online indicator in the top left and leave it as an icon, then add a button to turn on/off workspace syncing.

## Agent Chat
- [ ] Fix Devin auto-detect models
- [ ] Fix Cursor Agent auto-detect models
- [ ] Chat auto-naming
- [ ] **[Medium/Medium]** Permission approvals: show exact grant scope before
      resolving "Always allow" / "Allow for session", and add an optional
      broader "always allow this tool kind" scope tier.
  - *Plan: `docs/plans/other_tasks/pending-permission-grant-transparency-med-med.md`.*
  - *Today's "Always allow" silently caches `(tool, exact_target_or_command)`
    forever, with no preview and no tool-kind granularity — see
    `src/permissions/manager.rs::policy_key_for`.*