# Story: Browse Preview Tab + Preview Serving Route

> **Status:** pending | **Difficulty:** small
> **Epic:** [workspace-preview](../pending-workspace-preview-small.md). **Scope:** Go daemon + React frontend.

## Goal

Add a "browse preview" tab that renders a multi-file static site from the active
workspace inside the IDE, with relative asset paths (CSS, JS, images) resolving
correctly. Today the `HtmlViewer` only renders a single self-contained HTML file
because it loads `/api/workspaces/{id}/raw?path=index.html` — relative URLs lose
the query string and 404.

## Background / current behavior

- `internal/server/api.go:526` — `handleRawFile` serves a single file by
  `?path=` query param with proper Content-Type and range support. Relative
  URLs in an iframe at this URL resolve against `/api/workspaces/{id}/raw`
  (no path), so `./styles.css` → `/api/workspaces/{id}/raw` (wrong).
- `web/src/components/FileViewer.tsx:562` — `HtmlViewer` points an iframe at
  the raw URL with `sandbox="allow-same-origin"`. Works for self-contained
  HTML; breaks for any site with separate CSS/JS/asset files.
- `web/src/types/index.ts:42-63` — `Tab.kind` already supports `'file' |
  'settings'`; `viewMode` supports `'edit' | 'preview'`. Either can be extended.
- `WorkspaceMgr.FilePath` already does path-traversal containment and symlink
  rejection — the new route reuses it, no new security surface.

## Desired behavior

### Backend (Go)

- New route `GET /preview/{workspaceId}/{path...}` that serves files from the
  workspace root by path, backed by `http.FileServer` (or `http.ServeFile`)
  over `WorkspaceMgr.FilePath(ctx, workspaceID, relPath)`.
- Same `requireAuth` middleware as `/raw`; same `deviceId`/`secret` query-param
  credential fallback for browser tags that can't set Authorization headers.
- `Content-Disposition: inline` so the browser renders rather than downloads.
- Path-traversal (`..`) and symlink containment reuse `WorkspaceMgr.FilePath`
  — do not roll a new path validator.
- SPA-style fallback (serving `index.html` for missing paths) is **out of
  scope** for this story — serve 404 for missing files. Add later if needed.

### Frontend (React)

- Extend `Tab.kind` to include `'preview'` (or add a `previewEntry` field to
  the existing `file` kind — pick whichever is less invasive given the
  existing `viewMode: 'preview'` semantics; document the choice in the PR).
- Add a `BrowsePreview` component (or extend `HtmlViewer`) that points a
  sandboxed iframe at `/preview/{workspaceId}/{entryPath}`.
  - Sandbox: `allow-scripts allow-same-origin` (the workspace is the user's
    own code; this is the standard local-preview sandbox). `allow-top-navigation`
    stays OFF so the preview can't redirect the IDE.
- Entry point: "Open Preview" action on the file-tree context menu for
  `.html`/`.htm` files. Opens a preview tab keyed by the workspace + entry path.
- Preview tabs are persistent (not transient `isPreview` tabs) so the user can
  switch back to them; they are not editable.
- A refresh button on the preview tab (manual reload of the iframe) — live
  reload is a follow-on story.

## Acceptance criteria

- [ ] `GET /preview/{workspaceId}/index.html` serves the file with
      `Content-Type: text/html` and `Content-Disposition: inline`.
- [ ] A site with `index.html` referencing `./styles.css` and `./app.js`
      renders correctly in the preview tab (relative paths resolve).
- [ ] `GET /preview/{workspaceId}/../../../etc/passwd` is rejected (403/404)
      via `WorkspaceMgr.FilePath` containment — no new validator needed.
- [ ] Auth required on the preview route; loopback bypass works; remote LAN
      device with `deviceId`/`secret` query params works.
- [ ] "Open Preview" appears in the file-tree context menu for `.html`/`.htm`
      files and opens a persistent preview tab.
- [ ] Preview tab shows a manual refresh button; clicking it reloads the iframe.
- [ ] Sandbox is `allow-scripts allow-same-origin` (no `allow-top-navigation`);
      documented in a code comment explaining the choice.
- [ ] Server test: new route serves a file, rejects traversal, requires auth.
- [ ] Frontend smoke: preview tab renders an iframe pointed at the preview URL.
- [ ] `go test ./internal/server/...`, `go vet ./...`, `npm run build`,
      `npm run lint` all pass.

## Notes for implementor

- Reuse `handleRawFile`'s auth + credential-fallback pattern; the new handler
  differs only in taking the path from the URL path tail instead of a query
  param, so relative URLs resolve naturally.
- Do not add a new path validator — `WorkspaceMgr.FilePath` is the containment
  boundary. If it doesn't cover a case, fix it there, not in the handler.
- The `mockup*.html` files in the repo root are good test fixtures for a
  self-contained preview; build a small two-file fixture (index.html + style.css)
  for the relative-path test.
- Keep the frontend change minimal — a new component or a small extension to
  `HtmlViewer`, plus the context-menu entry. Do not refactor `FileViewer`'s
  dispatch in this story.

## Out of scope

- Live reload on file save (separate follow-on story).
- Dev-server proxy (Vite/Next.js) — much larger, separate epic or story.
- Auto-detection of `index.html` at workspace root.
- SPA client-side routing fallback (serve index.html on 404).
- Rust backend parity — track as a follow-on once the Go route + contract
  fixture land.
- Mobile-specific preview UX.
