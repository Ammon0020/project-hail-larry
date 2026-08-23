- name: apiFetch forces Content-Type: application/json on bodyless GET/DELETE requests
- file: /media/adam/extex/projects/project-hail-larry/web/src/lib/api.ts
- lines: 98-105
- description: |
    `apiFetch` unconditionally sets `'Content-Type': 'application/json'`
    (line 102) on every request, including GETs and DELETEs that have no
    body. This is harmless for the Go daemon's own handlers, but is
    technically incorrect and can break when a reverse proxy or
    middleware inspects `Content-Type` on GETs (some WAFs reject GETs
    with a JSON content type as suspicious). It also means
    `putMcpConfig` (line 541), which sends raw JSON text, correctly
    advertises JSON — but only by accident of this default.

    Fix: only set `Content-Type: application/json` when `options?.body`
    is present and the caller hasn't already supplied a Content-Type via
    `withAuthHeaders`. This also makes the `uploadFile` bypass (which
    must NOT set Content-Type so the browser can set the multipart
    boundary) less of a special case — the same rule would apply.
- verification: |
    Read apiFetch (lines 98-118). The headers object always includes
    `'Content-Type': 'application/json'` regardless of method or body
    presence. uploadFile (line 438) bypasses apiFetch specifically to
    avoid this, confirming the default is problematic for non-JSON bodies.
