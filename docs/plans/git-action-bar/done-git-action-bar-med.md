# S-GIT-ACTION-BAR — Git action bar item + status panel

> Story. Difficulty: medium. Urgency: medium. Epic: `pending-git-action-bar-large.md`.
> Depends on: S-GIT-DETECT, S-GIT-DIFF-VIEWER, S-GIT-API.

## Goal

A discoverable git surface in the IDE: an action bar item showing the current
branch and a compact change-count badge, opening a panel that lists changed
files, lets the user stage/unstage, commit, and push. Clicking any file opens
the reusable `GitDiffViewer` in an editor tab.

## Scope

- New action bar item in `ActivityBar.tsx` (top group, after Search):
  - Icon: `GitBranch` (from `lucide-react`, already a dep).
  - Only rendered when `repo_detected === true` (from S-GIT-DETECT's
    `useGitState` hook). No-repo workspaces never show the item.
  - Active state highlight matches the existing `ActivityBar` pattern.
  - Compact badge on the icon: change count (e.g. `3`) when > 0. Hidden
    when clean.
- New `web/src/components/git/GitPanel.tsx` (left sidebar panel, same
  pattern as `SearchPanel.tsx`):
  - Header: branch name + ahead/behind upstream chips (e.g. `↑2 ↓1`).
  - File list grouped by `Staged` / `Changes` / `Untracked`, each row shows
    status icon, path, and `+N / -M` diff stat (from `/status`).
  - Clicking a row opens `GitDiffViewer` for that path (staged vs. unstaged
    depending on the group).
  - Stage/unstage toggle per row (chevron or hover action).
  - Commit input + button at the bottom; disabled until ≥1 file staged and
    message non-empty. Sends `If-Match: <head_oid>` from last `/status`.
  - `Push` button in the header next to the ahead chip; opens a small
    modal streaming stderr on success/failure.
  - Refresh button; auto-refresh on workspace file-watcher events (debounced
    like the existing file-tree refresh).
- Mobile: panel is reachable via the bottom-nav (matches `MobileNav.tsx`
  pattern) — same content, narrower column.
- All write ops are client-initiated workspace writes (same auth model as
  file-save/rename/mkdir): authenticated paired-device gate, no permission
  prompt. The action bar fires them directly on click.

## Out of scope

- Branch switching UI.
- Pull/fetch buttons (S-GIT-API does not expose them).
- Stash, log history, blame.
- Diff stat calculation in the viewer — the panel computes/renders it.

## Acceptance

- [ ] Action bar item only appears on git repos; badge shows change count.
- [ ] Panel lists staged/changes/untracked with correct status icons and
      `+/-` stats.
- [ ] Clicking a file opens `GitDiffViewer` in an editor tab keyed by
      `path + baseOid + headOid`.
- [ ] Stage/unstage per row updates the list without a full reload.
- [ ] Commit refuses when no files staged or message empty; sends
      `If-Match`; on 409 refetches status and shows a toast.
- [ ] Push streams stderr to a modal; never stores credentials.
- [ ] Mobile: bottom-nav entry opens the panel in a narrow layout.
- [ ] Authenticated paired-device gate on write ops (no permission prompt —
      same model as file-save/rename/mkdir).
- [ ] Component tests for empty/clean/dirty/loading states.
- [ ] `make check` passes.

## Verification

- Vitest component tests for `GitPanel` states (clean, dirty, loading,
  error) and the action bar show/hide logic.
- Manual smoke: stage a file, commit, push to a local bare repo fixture,
  confirm ahead/behind chips update.

Suggested commit: `feat(git): action bar item + status panel (S-GIT-ACTION-BAR)`
