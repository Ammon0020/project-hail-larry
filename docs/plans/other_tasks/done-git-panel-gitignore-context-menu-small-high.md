# Git panel: .gitignore endpoint + row context menu

> Difficulty: small. Urgency: high.
> Source: `--untracked-files=all` regression + missing ignore UX (2026-07-28).

## Goal

Let users add files/folders to `.gitignore` directly from the git panel's file
rows, so untracked dirs like `target/` or `node_modules/` stop reappearing
after the recent switch to `git status --untracked-files=all`.

## Scope

### In scope

1. **Backend** — `POST /api/workspaces/{id}/git/ignore`:
   - `src/git/mod.rs`: `pub fn add_to_gitignore(root, patterns) -> Result<Vec<String>, GitError>`.
     Reads `root/.gitignore` (creates if missing), dedups exact-line matches,
     trims+validates non-empty patterns, appends new ones, returns the newly
     added list. Uses `GitError::Operation` for I/O errors (no repo needed).
   - `src/api/git.rs`: `pub async fn ignore_paths(...)` handler with
     `IgnoreRequest { patterns: Vec<String> }`; reuses `workspace_root` +
     `run_git_blocking`; returns `Json(json!({ "added": added }))`.
   - `src/api/mod.rs`: route next to the other git routes.
   - Tests in `src/git/mod.rs` `mod tests` (reuse `fresh_repo`):
     `gitignore_creates_file_when_missing`, `gitignore_dedups_existing_lines`,
     `gitignore_appends_new_patterns`.

2. **Frontend context menu** on each `GitFileRow`:
   - Wrap row in `DropdownMenu` with `DropdownMenuTrigger asChild`; right-click
     + long-press open the menu (mirror `FileTree.tsx`'s pattern; copy
     `useLongPressHandlers` into `GitPanel.tsx` with a comment noting the
     duplication).
   - Menu items: Open Diff (non-folder only), Stage/Unstage (by `file.staged`),
     Add to .gitignore (label "Add folder to .gitignore" for paths ending `/`),
     Copy Path. Separator between stage/unstage and ignore/copy groups.
   - Controlled menu state `menuPath` at `ChangeSection` level.
   - `onIgnore: (path: string) => void` prop on `GitFileRow` + `ChangeSection`,
     wired to `api.gitIgnore(workspaceId, [path])` via `runMutation('ignore', …)`
     in `GitPanel`. `refreshStatus` (already in `runMutation`) re-fetches so the
     ignored entry disappears.

3. **API client** — `web/src/lib/api.ts`:
   `gitIgnore(workspaceId, patterns)` POSTing `{ patterns }`, returning
   `{ added: string[] }`, next to the other git methods.

### Out of scope

- Bulk "ignore all untracked" action.
- Editing existing `.gitignore` lines from the UI.
- Persisting a per-row "ignored" flag in client state.

## Acceptance criteria

- [ ] `POST /api/workspaces/{id}/git/ignore` creates/updates `.gitignore`,
      dedups exact-line matches, returns `{ added: [...] }`.
- [ ] Empty/whitespace-only patterns are rejected with `GitError::Operation`.
- [ ] Three new rust tests pass (`gitignore_*`).
- [ ] Right-clicking a git row opens a context menu with the items above.
- [ ] Long-press on touch opens the same menu.
- [ ] "Add to .gitignore" calls the endpoint and the row disappears after the
      status refresh.
- [ ] Folder rows show "Add folder to .gitignore" and pass the trailing-slash
      path as-is.
- [ ] `make check` passes (fmt + clippy + cargo test + frontend eslint/build
      + contract suite).

## Verification

1. `make qcheck` — autofix fmt/lints + quiet tests.
2. `make check` — full gate.
3. Manual: stage/unstage, open diff, ignore a folder row, copy path.

## File references

- `src/git/mod.rs`, `src/api/git.rs`, `src/api/mod.rs`
- `web/src/components/git/GitPanel.tsx`, `web/src/lib/api.ts`
- Pattern source: `web/src/components/FileTree.tsx` (context menu + long-press)

## Depends on

None. Part 2 (virtualization) is independent and tracked separately.
