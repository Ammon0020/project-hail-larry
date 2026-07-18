# Epic: Workspace Static Preview

> **Status:** Partial — serve + tab + live-reload done; dev-server proxy open.
> **Owner:** —. **Created:** 2026-07-18. **Updated:** 2026-07-18.
> **Related:** Blueprint §13 (file serving), `src/api/mod.rs` (`preview_file` /
> `raw_file`), `web/src/components/BrowsePreview.tsx`.

## Goal

Let users open a "browse preview" tab that renders a multi-file static site from
the current workspace inside the IDE, with relative asset paths (CSS, JS, images)
resolving against the workspace root.

## Why a separate epic

New product capability (not rust-port parity). Touches backend route + frontend
tab kind, with clear follow-ons (live reload, dev-server proxy).

## Scope

**Done (S-PREVIEW-SERVE):**

- Backend `GET /preview/{workspace_id}/{*path}` with Content-Type + inline
  disposition, reusing `file_path` containment and the same auth as `/raw`.
- Frontend "Open Preview" on `.html`/`.htm` file-tree context menu.
- `Tab.kind: 'preview'` + `BrowsePreview` sandboxed iframe.
- Sandbox: `allow-scripts allow-same-origin` (no `allow-top-navigation`).

**Done (S-PREVIEW-LIVE-RELOAD):**

- Debounced iframe remount on `FileWritten` / `FileChangedOnDisk` for the
  preview workspace (shared `backend.events`, ~250ms).

**Still open:**

- Dev-server proxy (Vite, Next.js, etc.).
- Mobile-specific preview UX.
- Multi-entry workspace detection (auto-pick `index.html`).
- **Security follow-on:** isolate preview from IDE origin (see known-issues
  `sec-preview-same-origin-scripts`) — separate origin / no same-origin
  sandbox / scoped preview token.

## Architecture

`GET /preview/{workspaceId}/*` via `serve_workspace_file` (shared with `/raw`).
Iframe at `/preview/{workspaceId}/index.html` resolves `./styles.css` →
`/preview/{workspaceId}/styles.css`. Live reload remounts the iframe when WS
file events match the preview's `workspaceId`.

## Story Index

| Story | Title | Difficulty | Status |
|---|---|---|---|
| [S-PREVIEW-SERVE](workspace-preview/complete-browse-preview-small.md) | Browse preview tab + serving route | small | complete |
| [S-PREVIEW-LIVE-RELOAD](workspace-preview/complete-browse-preview-live-reload-small.md) | Live reload on file change | small | complete |
