# User-noted improvements

Users write the key missing features they see.

**Difficulty:** `Trivial` < `Small` < `Medium` < `Large`
**Urgency:** `High` > `Medium` > `Low`

Items are grouped by code area so a developer can stay in one context while working through a batch. Within each group, quick wins come first. Dependencies are noted inline.

---

## Quick wins (cleanup & polish)

- [x] **[Trivial/High]** Remove diff button. It doesn't make sense there. What would you diff against?
  - *Done 2026-07-12: Deleted button + `GitCompare` import from `TabBar.tsx`.*
- [x] **[Trivial/High]** Gray out save button unless edits have been made.
  - *Done 2026-07-12: `TabBar.tsx` — `canSave = !!activeTab?.unsaved`; muted/opacity-60 + `aria-disabled` when no edits.*
- [x] **[Trivial/High]** Add agent icon on the far right of the top bar, next to the save icon.
  - *Done 2026-07-12: `Bot` icon in `App.tsx` top bar. Now toggles the right chat panel (primary when visible, secondary when hidden) via `toggleRightPanel` added to `usePanelResize.ts`.*

## Harness & models

- [ ] **[Medium/High]** ⛔ **Blocked** Mistral asks for login every server restart. Should stay logged in.
  - *Investigated 2026-07-12: Auth is browser-PKCE per-session in `internal/acp/acp.go:478-493`. The ACP SDK's `AuthenticateResponse` only exposes `Meta map[string]any` — no tokens/cookies — so auth state is opaque inside the SDK's `Connection` and destroyed on daemon restart. `~/.vibe/.env` exists but is empty (no persisted key). Unblocking requires either (a) an ACP SDK change to expose persistable auth state, (b) a Mistral Vibe change to persist its own auth locally, or (c) a daemon-side long-lived auth cache that survives restarts (not possible while tokens are opaque). Logged in `docs/known-issues.md`.*
- [ ] **[Small/Medium]** ⛔ **Blocked** Devin CLI ACP (`devin acp`) — Automatically fetch available models.
  - *Investigated 2026-07-12: Devin has no programmatic model enumeration — no `--list-models` flag, no ACP `providers/list` support, and `~/.config/devin/config.json` stores only a single default `agent.model` string (not a list). Models are cloud-sourced and dynamic ("release frequently"). The current hardcoded fallback list (sourced from `--model` help-text examples) is the best local approach. Unblocking requires either a Cognition cloud API endpoint (needs auth + network) or a future Devin CLI flag. See `internal/acp/autodetect.go:117-143`.*
- [x] **[Small/Medium]** If models couldn't be fetched on a harness, show a warning icon by the harness name. If the user taps or hovers over the warning it shows the actual warning. Show an icon in the dropdown too.
  - *Done 2026-07-12: `WorkspaceBar.tsx` — `AlertTriangle` icon + tooltip (`relative z-10` so it's reachable over the overlay select); `(!)` prefix on dropdown options for warned agents.*

## Tabs

- [x] **[Small/Medium]** Add a bar right below the tabs to show a file's filepath, up to the workspace. So "workspace/folder/file.txt".
  - *Done 2026-07-12: New `BreadcrumbBar.tsx` component with `Folder` icon on the workspace segment + `ChevronRight` separators; wired into `EditorPane.tsx` (new `workspaceName` prop from `App.tsx`).*
- [x] **[Medium/Medium]** When opening a file from the explorer, italicize the tab's name. If they click on another file, replace the previous tab with the new file. This way if they click a ton of files trying to find something they don't end up with a hundred files. Think of the way vs code handles files. Then the moment they make a change, unitalicize and keep that open. Sort of like a buffer tab?
  - *Done 2026-07-12: `isPreview` field on `Tab`; `handleFileSelect` replaces the existing preview tab in-place; `handleContentChange`/`handleTabSelect` convert preview→persistent; italic styling in `TabBar.tsx`; preview tabs excluded from localStorage.*
- [x] **[Medium/Medium]** Add a tab right click menu. Close, close others, close saved, close to the right, copy path, copy relative path, keep open, etc. Mobile friendly.
  - *Done 2026-07-12: Controlled `DropdownMenu` with `DropdownMenuTrigger asChild` on each tab; right-click (desktop) + long-press 500ms (mobile). 7 actions wired in `App.tsx` + passed through `EditorPane.tsx` to both desktop and mobile TabBars. Note: Copy Path and Copy Relative Path both copy the relative path (absolute path needs a backend change to expose workspace root).*
- [ ] **[Large/Low]** Make tabs draggable. Mobile friendly (hold for a moment until highlighted then drag).
  - *No drag-and-drop lib in `web/package.json`. Needs a dep (e.g. `@dnd-kit/core`), reorder logic on the tabs array, and touch long-press-to-drag. Adds ~50KB bundle.*

## Git

- [ ] **[Large/Medium]** Add a git action bar item and diff viewer and a way to stage/commit/push if a git repo is detected.
  - *Backend only has `runGit()` for agent context (`internal/acp/context.go:407-425`); no git API endpoints in `internal/server/`. Frontend hardcodes "main" in `EditorPane.tsx:496`. Needs: status/diff/stage/commit/push endpoints, a diff viewer (`@codemirror/merge` not installed), and a git action bar UI. Foundational for the items below.*
- [ ] **[Small/Medium]** Add a way to initialize git in the workspace if it isn't detected.
  - *Depends on the git detection infrastructure above. Add `POST /api/workspaces/{id}/git/init` + a UI trigger (WorkspaceHeader/settings).*

## App-level UI

- [x] **[Small/Medium]** The bottom bar is only in the editor. It should span the entire bottom of the app.
  - *Done 2026-07-12: Extracted `StatusBar.tsx` component; lifted `fontSize` state to `App.tsx`. On desktop the status bar renders at the app level (full width, over the chat panel); on mobile it stays inside `EditorPane` above the bottom nav.*
- [ ] **[Medium/Medium]** Add a command palette and handle ctrl+p. Put the button to the right of the agent icon. An "Action items menu".
  - *`useKeyboardShortcuts` hook exists but has no Ctrl+P handler and no palette. Needs a command registry, a palette dialog component, and quick-open file search. Depends on the agent icon item above for button placement.*

## Chat

- [ ] **[Large/Medium]** Add a little popup/slideup above the chat input with icons for edited files and subagents and anything else we choose to add. Popup only shows if something is in one of them. The one that has a status has a blue dot indicator on its top right. Extremely compact. Eg. If a file is changed, it shows the blue dot by that and when you click it, the popup slides open more to reveal the list of edited files and changed line counts (plus and minus) on the right of the filename, and to the right of the edits is an accept edits button and a revert button. Revert requires a confirmation. When you click/tap any file it opens the agent diff in an editor tab.
  - *Popup pattern exists (`web/src/components/chat/McpPopout.tsx`); chat input is `ChatComposer.tsx`. Missing: edited-files list with +/- line counts, subagent status tracking, blue-dot indicator, accept/revert actions, and an agent diff viewer. Depends on the git diff viewer above for diff rendering.*
