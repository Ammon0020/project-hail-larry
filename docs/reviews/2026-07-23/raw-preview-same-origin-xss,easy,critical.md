# raw_file and preview_file serve attacker-controlled HTML in the IDE origin (same-origin XSS)

- **Difficulty:** easy
- **Urgency:** critical
- **File:** `src/api/mod.rs`
- **Lines:** 697-730 (serve_workspace_file), 741 (text/html content type), 673-693 (preview_file CSP)

## Description

`serve_workspace_file` is the shared body for `GET /api/workspaces/{id}/raw` and `GET /preview/{id}/{*path}`. It sets `Content-Type` from the file extension via `content_type_for_path`, which returns `text/html; charset=utf-8` for `.html`/`.htm` (line 741), and sets `Content-Disposition: inline` (line 713-716). Both routes live on the same daemon host/port as the IDE, so the response is **same-origin with the IDE**. An ACP agent (or any paired device) that can write a workspace file can drop `evil.html` containing `<script>fetch('/api/workspaces',{credentials:'include'})…</script>`. When any user opens `/api/workspaces/{id}/raw?path=evil.html` or `/preview/{id}/evil.html`, the script executes with full IDE-origin authority: it can read `document.cookie`, `localStorage`, and call every authenticated `/api/*` endpoint. The preview route adds only `Content-Security-Policy: frame-ancestors 'self'` (line 687-690) — that restricts *who can iframe* the response, not *what scripts in the response can do*. There is no `sandbox` CSP directive, no `script-src` restriction, and no `X-Content-Type-Options: nosniff` anywhere in the codebase. This is a complete sandbox escape: agent-written content runs as the IDE.

## Recommendation

Serve preview/raw from a separate origin (different host/port or a sandboxed subdomain) so scripts cannot reach IDE cookies or `/api/*`. At minimum, add `Content-Security-Policy: sandbox allow-same-origin` (or stricter) and `X-Content-Type-Options: nosniff` to every raw/preview response, and serve HTML previews with a restrictive `script-src 'none'` unless explicitly intended. Long-term, render previews through a per-session ephemeral origin.

## Verification

`content_type_for_path` (line 733-757) maps `html`/`htm` → `text/html; charset=utf-8`. `serve_workspace_file` (line 712-716) sets `Content-Disposition: inline` and only `Referrer-Policy` + `Content-Type` — no nosniff, no script CSP. `preview_file` (line 684-692) adds only `frame-ancestors 'self'`. `grep -rn 'X-Content-Type-Options|nosniff' src` returns zero matches. Both routes are mounted on the same Axum router as `/api/*` (lines 163, 171).
