# Epic: Git History Graph, Branch Switching, and Remote Sync

> **Status:** Pending. **Difficulty:** large. **Urgency:** medium.
> **Source:** user request (2026-07-28). **Created:** 2026-07-28.
> **Depends on:** `done-git-action-bar-large.md` (git API + panel surface).

## Goal

Add a VS Code-style commit history graph with branch/merge topology, plus
branch switching and fetch/pull — the features that make the git panel feel
complete. The graph reuses the existing `GitDiffViewer` for per-commit diffs.

## Why now

The git panel handles status/stage/commit/push, but there's no way to:
- See commit history or branch topology
- Switch branches from the UI
- Fetch/pull from a remote

Users currently need a terminal for these, breaking the self-contained IDE
promise. The graph is the most-requested remaining git feature.

## Architecture decisions (proposed)

- **Graph rendering:** SVG-based commit graph (not a third-party library).
  The topology is simple enough — vertical column per branch, dots for
  commits, lines for parent edges. Keeps the bundle small and the rendering
  controllable. Compare: git-log-graph libs (e.g. `gitgraph.js`) add ~50KB
  and have stale maintenance; a custom SVG is ~200 lines and matches the
  app's token system.
- **History data:** new backend endpoint `GET /api/workspaces/{id}/git/log`
  returns a paginated commit list with parent refs, branch labels, and HEAD.
  The frontend computes the column layout from the parent graph. No `git log
  --graph` parsing — the topology is derived from parent SHAs, which is more
  reliable than parsing ASCII graph output.
- **Branch switching:** `POST /api/workspaces/{id}/git/checkout` with branch
  name. Refuses if working tree is dirty (returns 409 with the dirty file
  list). No remote branch creation — checkout existing local branches only
  for v1.
- **Fetch/pull:** `POST /api/workspaces/{id}/git/fetch` and `.../git/pull`.
  Streams stderr like the existing push handler. No credential storage —
  uses the agent's environment git credentials (same model as push).
- **Virtualization:** the graph list virtualizes rows (e.g. `react-window`
  or a simple intersection-observer lazy-render) to handle repos with
  thousands of commits without rendering all rows.

## Stories

### S-GIT-LOG-API — Backend git log endpoint (medium)

`GET /api/workspaces/{id}/git/log?limit=100&offset=0` returns:
```json
{
  "commits": [
    {
      "oid": "abc123",
      "parents": ["def456"],
      "message": "commit subject",
      "author": { "name": "...", "email": "...", "time": "2026-07-28T..." },
      "branch_labels": ["main"],
      "is_head": true
    }
  ],
  "total": 1234,
  "has_more": true
}
```

- Uses `gix` to walk the commit graph (no `git log` CLI spawn, matching the
  existing architecture decision).
- `limit` capped at 200; `offset` for pagination.
- Branch labels: resolve which branches point at each commit (scan refs).
- HEAD: mark the commit HEAD points at.
- Out of scope: tag labels (separate story), search/filter.

### S-GIT-GRAPH-VIEWER — Commit graph component (medium)

New `web/src/components/git/GitHistoryTab.tsx`:
- SVG commit graph: one column per branch, dots for commits, curved lines
  for parent edges. Column assignment computed from the parent graph
  (greedy: assign each commit to its parent's column, or a new column for
  branch points).
- Each row: graph segment + short hash + message (truncated) + relative
  time + branch labels as colored chips.
- Click a commit → open `GitDiffViewer` with the commit's diff (parent vs
  commit). Reuses the existing diff tab infrastructure.
- Virtualized list for performance.
- Mode toggle: graph vs. flat list (for narrow mobile viewports).
- Reached from a "History" button in the GitPanel header.

### S-GIT-CHECKOUT — Branch switching (small)

- `POST /api/workspaces/{id}/git/checkout` with `{ branch: string }`.
- Backend: `git checkout <branch>` via CLI (gix doesn't support checkout
  write ops). Refuses if working tree dirty (409 + file list).
- Frontend: branch dropdown in GitPanel header (replaces the static branch
  display). Shows local branches, highlights current. On select → checkout
  → refresh status + file tree.
- No remote branch creation, no new branch creation for v1.

### S-GIT-FETCH-PULL — Fetch and pull from remote (small)

- `POST /api/workspaces/{id}/git/fetch` — `git fetch`, streams stderr.
- `POST /api/workspaces/{id}/git/pull` — `git pull`, streams stderr, refuses
  if dirty (409).
- Frontend: "Fetch" button in GitPanel header. Pull is a dropdown option on
  the fetch button (or a separate button if the ahead/behind chips indicate
  behind).
- Update `status()` to populate `upstream`/`ahead`/`behind` (currently
  returns zeros by design — this story implements the reference traversal).
- Same credential model as push (agent environment, no storage).

### S-GIT-TAGS — Tag display in graph (small, optional)

- Extend `S-GIT-LOG-API` to include tag labels in `branch_labels`.
- Render tags with a distinct chip color in the graph.
- No tag creation for v1 (display only).

### S-GIT-STASH — Stash support (small, optional)

- `POST /api/workspaces/{id}/git/stash` — `git stash push` with optional
  message.
- `POST /api/workspaces/{id}/git/stash/pop` — `git stash pop`.
- `GET /api/workspaces/{id}/git/stash/list` — list stash entries.
- Frontend: stash section in GitPanel (below the commit input), with
  stash/pop/drop actions per entry.

## Out of scope

- Interactive rebase UI (very complex, high maintenance burden).
- Merge conflict resolution UI (complex, rare in this workflow).
- Force push / history rewriting.
- Cherry-pick / revert from graph.
- Commit search/filter (can be added later as a search bar in the graph).
- Blame (separate surface — editor integration, not git panel).
- Remote management (add/remove/edit remotes).
- PR/MR integration.

## Acceptance (per story)

Each story has its own acceptance criteria in its story file. Epic-level:
- [ ] Commit history visible as a graph with branch topology.
- [ ] Clicking a commit opens its diff.
- [ ] Branch switching works from the UI.
- [ ] Fetch/pull works and updates ahead/behind chips.
- [ ] Graph virtualizes for repos with 1000+ commits.
- [ ] Mobile: graph collapses to flat list on narrow viewports.
- [ ] `make check` passes for all stories.

## Suggested order

1. S-GIT-LOG-API (backend, no UI)
2. S-GIT-GRAPH-VIEWER (frontend, depends on log API)
3. S-GIT-CHECKOUT (small, high value)
4. S-GIT-FETCH-PULL (small, completes the remote loop)
5. S-GIT-TAGS (optional)
6. S-GIT-STASH (optional)
