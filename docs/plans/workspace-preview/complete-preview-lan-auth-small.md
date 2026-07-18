# Story: LAN Preview Relative-Asset Authentication

> **Status:** complete | **Difficulty:** small
> **Epic:** [workspace preview](../complete-workspace-preview-small.md)

## Implementation

- Added authenticated `POST /api/workspaces/{id}/preview-session`.
- Tickets are 256-bit random, workspace-bound, in-memory, and expire after
  30 minutes.
- Preview entry URLs consume a ticket and exchange it for a fresh HttpOnly,
  `Path=/preview/{workspaceId}/` cookie.
- Preview middleware accepts only the matching unexpired ticket or cookie;
  device credentials and loopback behavior remain unchanged.
- `BrowsePreview` obtains a ticket before mounting its `allow-scripts`-only
  iframe. Device secrets are no longer included in preview URLs.

## Security notes

The short-lived entry ticket can appear in browser history or server logs;
preview responses retain `Referrer-Policy: no-referrer`. Cookies are
`SameSite=Lax`, HttpOnly, and receive `Secure` on the daemon's native TLS
listener. Preview content remains untrusted opaque-origin HTML/JS and can
make outbound requests.

## Verification

- `cargo test -q --lib api::`
- `cd web && npm run build --silent`
- LAN phone smoke test: pair a phone, open an HTML preview containing a
  relative CSS file, and confirm both document and stylesheet load.
