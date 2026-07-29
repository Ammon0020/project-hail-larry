# Plan — S-GIT-PANEL-FIXUP (git panel "non-functional")

> Implementation plan for `pending-git-panel-nonfunctional-medium-high.md`.
> Investigation complete. The panel is **mostly functional**; the dominant
> defect is the untracked-file diff/UX (item 1). Items 2–4 are wired
> correctly and were confirmed working — the "non-functional" perception is a
> consequence of item 1 plus a non-obvious fresh-init affordance.

## 1. Root cause per item

### Item 1 — Untracked-file diff UX  ·  **REAL DEFECT (UX, not a crash)**

- `git status --porcelain=v1 -z` after `git init` + the empty initial commit
  reports every pre-existing file as `??` → mapped to `untracked` in
  `src/git/mod.rs:298-304`. This is correct git behaviour, **not** a status
  bug. Verified empirically: a fresh repo with two files reports
  `?? a.txt` / `?? b.txt`.
- `diff()` for an untracked file, `staged=false`
  (`src/git/mod.rs:344-386`):
  - `base = git_file(root, ":path")` → `git show :a.txt` exits 128
    (`fatal: path 'a.txt' exists on disk, but not in the index`).
    `git_file` swallows non-zero exit and returns `Ok(Vec::new())`
    (`src/git/mod.rs:562-567`), so **`base = ""`**.
  - `head = std::fs::read(path)` → the full file contents.
  - Result: `DiffResult { base: "", head: <whole file>, truncated: false }`.
- `GitDiffViewer` renders `unifiedMergeView({ original: "" })` over
  `doc = head` (`GitDiffViewer.tsx:139-155`), i.e. the **entire file shown as
  added**. This is technically correct and legible, **but** there is no
  indication it is a brand-new/untracked file, and because *every* file is
  untracked post-init the whole panel reads as "everything is broken / every
  file changed."
- Edge case: an untracked **empty** file yields `base=""` and `head=""` → a
  blank merge view that genuinely looks broken.

**Conclusion:** No backend bug. The defect is presentation: an all-untracked
post-init repo with bare full-file "added" diffs and no "new file" label.

### Item 2 — Diff tab open path  ·  **NOT A BUG (verified reachable)**

Full trace:
`GitFileRow` `onClick={() => onOpenDiff(file.path, file.staged)}`
(`GitPanel.tsx:54`) → `ChangeSection` passes `onOpenDiff` through
(`GitPanel.tsx:114`) → `GitPanel` prop `onOpenDiff` (`GitPanel.tsx:127,268-269`)
→ `LeftSidebar` prop `onOpenDiff` (`LeftSidebar.tsx:141`) → `App.handleOpenDiff`
(`App.tsx:790-815`, wired at `App.tsx:1108`).

`handleOpenDiff` creates a `kind: 'git-diff'` tab with `staged` and activates
it (`App.tsx:799-812`). `EditorPane` renders `GitDiffTab` for every
`kind === 'git-diff'` tab (`EditorPane.tsx:583-594`). The `Tab` type declares
both `kind: 'git-diff'` and `staged` (`types/index.ts:57,78`), so this is
type-correct and builds. `GitDiffTab` fetches via `api.getGitDiff`
(`GitDiffTab.tsx:45`) with a stale-fetch guard and renders `GitDiffViewer`.

**Conclusion:** Click handler, tab kind, dispatch, and activation are all
correct. The user's "can't interact at all" is explained by item 1 — they
*did* get a diff tab, but it was a bare full-file green view with no label, so
it read as broken/useless. No code change required for the open path.

### Item 3 — Post-init refresh  ·  **NOT A BUG (verified transition works)**

`GitPanel` init button runs inside `runMutation('init', …)`
(`GitPanel.tsx:205-209`):
`api.gitInit()` → `refreshGitState()` (the panel's own `useGitState.refresh`)
→ `onRepoChanged()` (App-level refresh).

- `refreshGitState()` calls `api.getGitState` and `setGitState(...)`
  (`useGitState.ts:23`) → `gitState.repoDetected` flips `false → true` → the
  init screen (`GitPanel.tsx:194`) is replaced by the file-list view.
- The file list then populates via the effect
  `useEffect(() => refreshStatus(), [refreshStatus])` (`GitPanel.tsx:153-156`):
  `refreshStatus` is memoized on `[gitState?.repoDetected, workspaceId]`
  (`GitPanel.tsx:151`), so when `repoDetected` becomes `true` its identity
  changes, the effect re-runs, and `api.getGitStatus` fetches the untracked
  file list.
- `busyAction` is cleared in `finally` (`GitPanel.tsx:167-169`); no stuck
  loading state.

Latent (harmless) wrinkle: the trailing `await refreshStatus()` *inside*
`runMutation` (`GitPanel.tsx:163`) executes the **stale** `refreshStatus`
captured when `repoDetected` was still `false`, so that particular call
early-returns `setStatus(null)` (`GitPanel.tsx:141-144`). It does not matter
because the dependency-driven effect above refetches immediately after. Not
worth changing.

**Conclusion:** Transition init-screen → file-list works; no stuck state.

### Item 4 — Stage / commit reachability  ·  **NOT A BUG (verified end-to-end)**

- Per-file stage: `onStage([path], false)` → `runMutation('stage', …)` →
  `api.gitStage(id, [path], false)` (`GitPanel.tsx:268`) → POST
  `{paths:[path], all:false}` → handler passes the `!all && paths non-empty`
  guard (`api/git.rs:128`) → `git::stage` runs `git add -- <path>`
  (`git/mod.rs:408-416`).
- Stage-all: `onStage([], true)` → `{paths:[], all:true}` → handler sets
  `paths = []` (`api/git.rs:132-136`) → `git::stage` with empty paths runs
  `git add -A` (`git/mod.rs:399-407`).
- Commit: enabled when `message.trim()` and ≥1 staged file
  (`GitPanel.tsx:174`); `api.gitCommit` sends the `If-Match: headOid`
  precondition; `git::commit` verifies HEAD, injects a default identity when
  none is configured, and commits (`git/mod.rs:453-494`). The fresh-init empty
  commit means HEAD exists, so `status.headOid` matches and the commit
  succeeds.

**Conclusion:** Stage → commit → refresh works. The user's "stage/commit
aren't there" is a consequence of items 1/2: on a non-git folder only the
Initialize screen shows (correct), and post-init the untracked flood + bare
diffs made the working stage/commit UI feel absent/untrustworthy.

## 2. Untracked-file UX decision

Options considered:

- **(a) Keep the full-file diff, add a "New file" badge/header.** The viewer
  already renders `base=""` as an all-added file legibly; we only add a header
  label inferred from base/head emptiness. Tiny, no new plumbing, no backend
  or API change, and the diff stays useful (you can read the new file's
  contents).
- **(b) Replace the diff with a "New file — stage to diff" placeholder for
  untracked rows.** Hides content the user may want to read, adds a
  special-case branch in the open path, and diverges from how tracked files
  behave. More code, less useful.
- **(c) Add an obvious "Stage All" call-to-action for the all-untracked
  state.** Addresses the "every file changed" feeling but does nothing for the
  bare, unlabeled diff of an individual file.

**Recommendation: (a) as the primary fix, plus a light touch of (c).**
VS Code does exactly this — untracked files sit under "Changes" with a `U`
decoration and a group-level "Stage All Changes" (+), and opening one shows
the full file content. (a) is the least code and most maintainable (it lives
entirely in the presentation-only `GitDiffViewer`, matching its existing
"decoupled from the API" contract and its own doc note about the
"added file (base empty)" case). (c) is *already 90% built* — each section
header has a "Stage All Changes" (+) button (`GitPanel.tsx:99-108`); it just
needs to be more discoverable for the fresh-init case. (b) is rejected.

## 3. Concrete fix list (minimal — no refactors)

- **FIX 1 — "New file"/"Deleted file" badge in the diff header.**
  File: `web/src/components/git/GitDiffViewer.tsx` (header block ~line 168-169).
  Change: derive a label from the props —
  `base === '' && head !== ''` → `New file`;
  `head === '' && base !== ''` → `Deleted file`;
  `base === '' && head === ''` → `Empty file`; otherwise none. Render it as a
  small badge next to the `path` span in the existing header row.
  Est: ~10-14 lines (a `const` + one conditional `<span>`).
  Addresses: **Item 1** (legibility of untracked/added and deleted diffs;
  fixes the blank-pane empty-file edge case). No backend/API change.

- **FIX 2 — Make "Stage All" obvious for the unstaged section.**
  File: `web/src/components/git/GitPanel.tsx` (`ChangeSection` header,
  ~line 97-109). Change: for the unstaged ("Changes") section, surface the
  existing stage-all action as a visible "Stage All" text button (keep the
  icon for the staged/unstage-all case). Optionally add a one-line muted hint
  when the repo has only untracked files and no staged files, e.g.
  "New files — stage them, then commit." Keep it to the existing
  `onStage([], true)` handler; no new state.
  Est: ~8-12 lines.
  Addresses: **Item 1 / option (c)** (fresh-init "every file is new" reads as
  actionable, not broken).

- **Items 2, 3, 4 — no code change.** Verified working (see §1). Do not
  refactor the `handleOpenDiff` path, `useGitState`, or the stage/commit
  wiring.

Total: ~2 files, ~20-25 lines. No new endpoints (reuses S-GIT-API), no
backend change, no diff-renderer swap.

## 4. Verification steps

Build once (`./build.sh`) and run the daemon against a scratch workspace.

1. **Untracked diff legibility (FIX 1).** In a folder with files, click
   *Initialize Repository*. Confirm the file list appears with `U` badges.
   Click a file row → a "Diff: <name>" tab opens in the editor showing the
   whole file with a **"New file"** header badge. Delete a committed file and
   open its diff → **"Deleted file"** badge, `head` empty. Modify a tracked
   file and open its diff → no badge, normal added/removed markers. Open an
   empty untracked file → **"Empty file"** badge, no blank/confusing pane.
2. **Fresh-init actionability (FIX 2).** After init, confirm a visible
   "Stage All" affordance on the Changes section; click it → all files move to
   "Staged Changes". Confirm the optional hint shows only in the
   all-untracked / nothing-staged state.
3. **Stage → commit → refresh (item 4 regression).** Stage one file, type a
   message, Ctrl+Enter (or Commit). Confirm the list updates (staged file
   clears; remaining untracked stay). Confirm the branch/HEAD header updates.
4. **Non-repo guard (item 3 regression).** Open a non-git folder → only the
   "Initialize Repository" screen shows, no file list, no commit box.
5. **Diff tab dispatch (item 2 regression).** Reopen a diff tab for the same
   file → focuses the existing tab (no duplicate). Switch tabs mid-load → no
   wrong-file flash (stale-fetch guard).
6. `make check` passes (fmt + clippy + cargo test + eslint/build + contract).

## 5. Out of scope

- Backend `git/mod.rs`, `api/git.rs`, and `lib/api.ts` — **do not touch**;
  status/diff/stage/commit are correct. No new endpoints.
- `App.handleOpenDiff`, `EditorPane` tab dispatch, `useGitState`, and the
  stage/commit/push wiring — verified working; no refactor.
- The harmless stale-closure trailing `refreshStatus()` in `runMutation`
  (`GitPanel.tsx:163`) — leave as is (the dependency-driven effect covers it);
  changing it is not needed to satisfy acceptance.
- Branch switching, pull/fetch, stash, log, blame, and hunk-level
  staging/reverting — out per the epic.
- Migrating the diff renderer — `@codemirror/merge` stays.
- Upstream/ahead/behind population (`status()` returns zeros by design for the
  MVP, `git/mod.rs:325-334`) — unrelated to this story.
