# S-GIT-DIFF-VIEWER — Reusable editor git diff viewer

> Story. Difficulty: medium. Urgency: medium. Epic: `pending-git-action-bar-large.md`.
> Depends on: S-GIT-API.

## Goal

A single reusable diff component that any caller (action bar, future
edited-files popup, chat) can open with `path + base + head` and get a
CodeMirror-backed unified/side-by-side diff in an editor tab.

## Scope

- Add `@codemirror/merge` to `web/package.json` (CodeMirror 6 family; reuses
  existing language packs, theme, and the `uiw/react-codemirror` mount
  pattern).
- New `web/src/components/git/GitDiffViewer.tsx`:
  - Props: `path: string`, `base: string`, `head: string`, `mode?: 'unified' |
    'split'` (default `unified`, toggle in the viewer header), `language?:
    string` (auto-detected from `path` via the existing language loader).
  - Mounts `@codemirror/merge`'s `MergeView` through the existing CodeMirror
    theme/editor settings (font size, word wrap, tab size from
    `lai:*` localStorage keys — same source as `EditorPane`).
  - Read-only; no inline edit of the diff. Editing stays in the normal
    editor tab.
- Register a new editor tab kind `"git-diff"` in the existing tab state
  (`tabPreviewState.ts` / `EditorPane.tsx` tab dispatcher). A diff tab is
  keyed by `path + baseOid + headOid` so reopening the same file reuses the
  tab.
- `web/src/lib/api.ts`: add `fetchGitDiff(workspaceId, path, staged)` helper
  that calls `GET /diff` and returns the `{ unified, base, head, truncated }`
  payload. The viewer takes `base`/`head` strings; the caller decides whether
  to fetch from the API or pass agent-diff content (future chat popup).
- Truncation banner: when `truncated === true`, render a non-blocking banner
  above the viewer: "Diff truncated at <cap>."
- Mobile: split mode collapses to unified on narrow viewports
  (`@container` query, matching the existing `EditorPane` responsive pattern).

## Out of scope

- Inline editing of the diff (separate future story; editing is in the
  normal editor tab).
- Three-way merge conflict resolution UI.
- Diff stat header (+/- counts) — the action bar panel renders those
  separately from the viewer.

## Library

`@codemirror/merge` — sibling to the already-installed `@codemirror/state`,
`@codemirror/view`, `@codemirror/language`, etc. Pin to a version ≥7 days old
at impl time per repo policy. No new vendor lock-in: it's the same CodeMirror
6 family the editor already uses.

## Acceptance

- [ ] `GitDiffViewer` renders an added, modified, deleted, and renamed file
      correctly in unified and split modes.
- [ ] Reuses existing `lai:*` editor settings (font size, wrap, tab size).
- [ ] Diff tab is keyed by `path + baseOid + headOid` so reopening reuses it.
- [ ] `truncated` banner renders when the API caps the diff.
- [ ] Mobile collapses split → unified on narrow viewports.
- [ ] Read-only — no inline edits leak into the working tree.
- [ ] Component test covers the four file-status cases + truncation.
- [ ] `npm run lint --silent` + `npm run build --silent` pass.

## Verification

- Vitest component test rendering each status with mock `base`/`head` strings.
- Manual: open a changed file from the action bar (once S-GIT-ACTION-BAR
  lands) and confirm it routes through the same component.

Suggested commit: `feat(git): reusable CodeMirror merge diff viewer (S-GIT-DIFF-VIEWER)`
