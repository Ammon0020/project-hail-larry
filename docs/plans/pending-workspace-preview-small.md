# Epic: Workspace Static Preview

> **Status:** Pending. **Owner:** —. **Created:** 2026-07-18.
> **Related:** Blueprint §13 (file serving), `internal/server/api.go` (`handleRawFile`),
> `web/src/components/FileViewer.tsx` (`HtmlViewer`).

## Goal

Let users open a "browse preview" tab that renders a multi-file static site from
the current workspace inside the IDE, with relative asset paths (CSS, JS, images)
resolving against the workspace root. The IDE already runs in a browser; this
turns it into a lightweight preview surface for the user's own HTML/CSS/JS
projects without leaving the app.

## Why a separate epic

This is a new product capability. It does not fit under:

- `rust-port/` — that epic explicitly scopes out new product capabilities
  (behavior-preserving port only).
- `pending-acp-agent-session-history-med.md` — agent-side session persistence, unrelated.
- `complete-ui-library-evaluation-med.md` — closed decision record, no implementation stories.

It touches both backend (a new serving route) and frontend (a new tab kind),
and has a clear set of follow-on stories (live reload, dev-server proxy), so it
warrants its own epic.

## Scope

**In scope (this epic):**

- A backend route that serves workspace files by path with correct Content-Type
  and relative-path resolution, reusing the existing path-traversal / symlink
  containment from `WorkspaceMgr.FilePath`.
- A frontend "Open Preview" entry point (file-tree context menu on `index.html`
  or any HTML file, plus a tab action) that opens a preview tab.
- A preview tab kind that renders the site in a sandboxed iframe pointed at the
  new route.
- Sandbox policy decision documented (scripts + same-origin vs. same-origin only).

**Out of scope (future stories in this epic):**

- Live reload on file save (WebSocket file-change → iframe refresh).
- Dev-server proxy (Vite, Next.js, etc.) — much larger; needs process
  lifecycle, port management, and reverse-proxy wiring.
- Mobile-specific preview UX.
- Multi-entry workspace detection (auto-pick `index.html`).

## Architecture direction

Add a virtual preview root route, e.g. `GET /preview/{workspaceId}/*`, backed by
`http.FileServer` over the workspace directory (via `WorkspaceMgr.FilePath` for
containment). An iframe at `/preview/{workspaceId}/index.html` then resolves
`./styles.css` → `/preview/{workspaceId}/styles.css` naturally.

This reuses the auth + path-containment infrastructure already built for
`handleRawFile` (`GET /api/workspaces/{id}/raw`), but serves from a path-based
root rather than a query-string `path` parameter so relative URLs work.

**Sandbox policy:** the workspace is the user's own code, so
`sandbox="allow-scripts allow-same-origin"` is acceptable for a preview. This
should be a documented, deliberate choice in the story — combining both is the
standard "run untrusted-but-local preview" sandbox. `allow-top-navigation` stays
off so the preview can't redirect the IDE itself.

## Story Index

| Story | Title | Difficulty | Depends on |
|---|---|---|---|
| [S-PREVIEW-SERVE](workspace-preview/pending-browse-preview-small.md) | Browse preview tab + serving route | small | — |

## Open questions

1. **Route path** — `/preview/{workspaceId}/*` vs. reusing `/api/workspaces/{id}/raw`
   with a path-prefix mode. The former is cleaner for relative resolution; the
   latter avoids a new top-level route but needs URL rewriting.
2. **Auth on preview route** — same `requireAuth` + query-param credential
   fallback as `/raw`, or a per-workspace preview token? Default: match `/raw`.
3. **Entry-point discovery** — auto-detect `index.html` at workspace root, or
   require the user to open a specific HTML file? Default: user-driven; auto-detect
   is a follow-on.
4. **Rust port parity** — the Rust backend (`src/`) should gain the same route.
   Track as a follow-on story once the Go implementation lands and the contract
   fixture is captured.
