# S-GIT-PANEL-FIXUP — Git panel reported non-functional

> Story. Difficulty: medium. Urgency: high. Epic: `done-git-action-bar-large.md` (reopened follow-up).
> Status: Done (2026-07-28, commit 749c0b1). See `plan-git-panel-fixup.md` for investigation.
> Reported: 2026-07-28 (user testing).

## Symptom

User reports the Source Control surface is non-functional in practice:
- "I can't interact with the git diff at all" — clicking changed-file rows
  does not produce a usable diff.
- "It thinks literally every file is changed" — on a freshly-initialized
  repo every workspace file shows up in the change list.
- "On a folder that hasn't been initialized by git it should show only an
  option to initialize git. It should also let me stage and commit changes.
  Those aren't there right now."

The init-only screen and the stage/unstage/commit UI all exist in
`GitPanel.tsx`, so this is a runtime/UX bug, not missing features.

## Root-cause investigation needed

1. **Untracked-file diff UX.** After `git init` on a folder with existing
   files, `git status --porcelain=v1 -z` correctly reports every file as
   `??` (untracked) — that is expected git behavior, not a status bug.
   But `diff()` (`src/git/mod.rs:344`) for an untracked file returns
   `base=""` (index has no entry → `git show :path` fails → empty) vs
   `head=<file contents>`, so the merge view shows the entire file as
   added. That is technically correct but, combined with every file being
   untracked, makes the panel feel broken. Decide: (a) show a "New file"
   badge instead of a full-file diff for untracked rows, or (b) keep the
   diff but make the all-untracked post-init state legible.
2. **Diff tab open path.** Verify `handleOpenDiff` (`App.tsx:790`) actually
   opens a reachable `git-diff` tab and that `EditorPane` renders
   `GitDiffTab` for it. The user reports no interaction at all — confirm
   whether the click handler, tab kind, or tab activation is broken.
3. **Post-init refresh.** After `gitInit`, `GitPanel` calls
   `refreshGitState()` + `onRepoChanged()`. Confirm `useGitState` flips
   `repoDetected` → `true` and `refreshStatus` runs so the file list
   appears. A stale `loading`/`gitState` could leave the panel on the init
   screen.
4. **Stage/commit reachability.** The UI is present but the user perceives
   it as missing — likely a consequence of (1)/(2)/(3). Confirm staging an
   untracked file (`git add`) works end-to-end and the commit button
   enables.

## Scope

- Investigate and fix the four items above.
- Improve the post-`git init` UX so a folder full of untracked files is
  clearly "these are new files you can stage," not "everything is broken."
- Consider a "Stage All" affordance that is obvious for the fresh-init case.
- No new endpoints; reuse the existing S-GIT-API surface.

## Out of scope

- Branch switching, pull/fetch, stash, log, blame (still out per epic).
- Migrating the diff renderer — `GitDiffViewer` already uses
  `@codemirror/merge`; no library change needed.

## Acceptance

- [ ] Clicking a changed-file row opens a legible diff tab (or a clear
      "new file" view for untracked files).
- [ ] After `git init` on a folder with existing files, the panel clearly
      communicates the files are untracked and stageable.
- [ ] Stage → commit → (refresh) works end-to-end from the UI.
- [ ] Non-repo folders still show only the "Initialize Repository" screen.
- [ ] `make check` passes.

## Verification

- Manual smoke: `git init` a folder with files, open the panel, stage one
  file, commit, confirm the list updates.
- Manual smoke: open a diff for a modified (tracked) file and an untracked
  file; both should be legible.

Suggested commit: `fix(git): make source control panel functional post-init (S-GIT-PANEL-FIXUP)`
