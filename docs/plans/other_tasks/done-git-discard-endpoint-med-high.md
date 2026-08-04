# Git Discard Endpoint

## Problem

The git panel's "Discard Changes" button uses a fragile client-side workaround:
`readFile` → `getGitDiff` → `saveFile(base)`. This has several failure modes:

- **Deleted files**: `readFile` fails (file doesn't exist), `.catch()` swallows
  the error, then `getGitDiff` may also fail. Discard silently does nothing.
- **Binary files**: `saveFile` sends string content — binary files are corrupted.
- **Revision conflicts**: the `saveFile` revision handshake can reject if a
  concurrent edit bumps the revision between `readFile` and `saveFile`.
- **No confirmation**: clicking the destructive button immediately writes to
  disk with no undo. VS Code shows a confirmation dialog.

## Scope

1. **Backend**: Add `POST /api/workspaces/{id}/git/discard` accepting
   `{ paths: string[] }`. Internally runs `git checkout -- <paths>` for
   tracked files and `rm` for untracked files (matching VS Code behavior).
   Return `{ discarded: number }`.
2. **Frontend**: Add `api.gitDiscard(workspaceId, paths)` in `api.ts`.
3. **Frontend**: Replace the `readFile→getGitDiff→saveFile` workaround in
   `GitPanel.tsx` with a single `api.gitDiscard()` call.
4. **Frontend**: Add a confirmation dialog before discarding ("Discard changes
   to {filename}? This cannot be undone.").
5. **Security**: Same trust model as stage/unstage — paired device or loopback,
   workspace-scoped path containment via `workspace_root`.

## Acceptance Criteria

- [ ] `POST /api/workspaces/{id}/git/discard` restores tracked files and
      deletes untracked files.
- [ ] Path containment is enforced (no escaping workspace root).
- [ ] Frontend shows confirmation before discard.
- [ ] Binary and deleted files discard correctly.
- [ ] Contract test or unit test covers the endpoint.

## Verification

- `cargo test` passes with new unit tests in `src/git/mod.rs`.
- Manual test: modify a file, click discard, confirm dialog appears, file
  reverts. Repeat with an untracked file (deleted on discard).
