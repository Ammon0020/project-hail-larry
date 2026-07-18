# Story: Browse Preview Tab + Preview Serving Route

> **Status:** complete | **Difficulty:** small
> **Epic:** [workspace-preview](../complete-workspace-preview-small.md). **Scope:** Rust
> daemon + React frontend (Go deleted).

## Goal

Add a "browse preview" tab that renders a multi-file static site from the active
workspace inside the IDE, with relative asset paths (CSS, JS, images) resolving
correctly. Today the `HtmlViewer` only renders a single self-contained HTML file
because it loads `/api/workspaces/{id}/raw?path=index.html` — relative URLs lose
the query string and 404.

## Background / current behavior

- `src/api/mod.rs` — `raw_file` serves a single file by `?path=` query param with
  proper Content-Type. Relative URLs in an iframe at this URL resolve against
  `/api/workspaces/{id}/raw` (no path), so `./styles.css` → wrong.
- `web/src/components/FileViewer.tsx` — `HtmlViewer` points an iframe at the raw
  URL with `sandbox="allow-same-origin"`. Works for self-contained HTML; breaks
  for any site with separate CSS/JS/asset files.
- `web/src/types/index.ts` — `Tab.kind` supports `'file' | 'settings' | 'preview'`;
  `viewMode` / `isPreview` remain distinct (edit-mode preview vs transient tabs).
- `WorkspaceManager::file_path` already does path-traversal containment and
  symlink rejection — the preview route reuses it.

## Desired behavior

### Backend (Rust)

- Route `GET /preview/{workspace_id}/{*path}` serves files from the workspace
  root by path (same auth as `/raw`).
- `Content-Disposition: inline`; MIME via `content_type_for_path`.
- Path-traversal / symlink containment via `file_path` — no new validator.
- No SPA fallback for missing paths (404).

### Frontend (React)

- `Tab.kind === 'preview'` for browse-preview tabs (distinct from
  `viewMode: 'preview'` / `isPreview`).
- `BrowsePreview` iframe at `/preview/{workspaceId}/{entryPath}` with
  `sandbox="allow-scripts allow-same-origin"` (no `allow-top-navigation`).
- File-tree context menu "Open Preview" on `.html`/`.htm`.
- Manual Refresh remounts the iframe; live reload is a follow-on story
  (`complete-browse-preview-live-reload-small.md`).

## Acceptance criteria

- [x] `GET /preview/{workspaceId}/index.html` serves the file with
      `Content-Type: text/html` and `Content-Disposition: inline`.
- [x] Relative paths (`./styles.css`) resolve under `/preview/{id}/…`.
- [x] Traversal (`../`) rejected via `file_path` containment.
- [x] Auth required on the preview route (non-loopback unit test); loopback
      bypass + query-param credentials match `/raw` middleware.
- [x] "Open Preview" on `.html`/`.htm` opens a persistent preview tab.
- [x] Manual refresh button remounts the iframe.
- [x] Sandbox documented in code comment.
- [x] Server tests: serves HTML, rejects traversal, requires auth off-loopback.
- [x] `cargo test -q --lib api::`, `cargo clippy`, `npm run build` pass.

## Out of scope

- Live reload on file save (see `complete-browse-preview-live-reload-small.md`).
- Dev-server proxy (Vite/Next.js).
- Auto-detection of `index.html` at workspace root.
- SPA client-side routing fallback.
- Mobile-specific preview UX.
