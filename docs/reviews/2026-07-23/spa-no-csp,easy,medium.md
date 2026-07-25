# No Content-Security-Policy on the SPA shell

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `web/index.html`
- **Lines:** 1-13

## Description

`index.html` has no `<meta http-equiv="Content-Security-Policy">` tag, and the backend's global `security_headers` middleware (`src/api/mod.rs:286-302`) sets only `X-Content-Type-Options`, `X-Frame-Options`, and HSTS — no `Content-Security-Policy`. Without a CSP, any future XSS (e.g. from a regression in the DOCX/XLSX `dangerouslySetInnerHTML` paths in `FileViewer.tsx:349,420`, or a `rehype-raw` accidentally added to the chat markdown renderer) has no defense-in-depth barrier and can exfiltrate the localStorage device secret (device-credential-localstorage) or make arbitrary authenticated requests. A CSP of `default-src 'self'; script-src 'self'; connect-src 'self' ws: wss:; img-src 'self' data: blob:; style-src 'self' 'unsafe-inline'; frame-src 'self'` would meaningfully raise the bar.

## Recommendation

Add a CSP `<meta>` tag in `index.html` (or, preferably, set it as a response header in `security_headers` so it cannot be stripped by an HTML injection). Start restrictive and loosen only what is required (CodeMirror uses inline styles, so `style-src 'self' 'unsafe-inline'` is likely needed; blob/data URLs are needed for image previews).

## Verification

`index.html:1-13` — no `<meta http-equiv="Content-Security-Policy">`. `src/api/mod.rs:286-302` — `security_headers` inserts only `X-Content-Type-Options`, `X-Frame-Options`, `STRICT_TRANSPORT_SECURITY`. `grep -rn "Content-Security" /media/adam/extex/projects/project-hail-larry/src/` returns no matches.
