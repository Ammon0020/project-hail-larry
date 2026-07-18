# Story: Preview Sandbox Hardening

> **Status:** complete | **Difficulty:** small
> **Epic:** [workspace-preview](../complete-workspace-preview-small.md).

## Goal

Prevent workspace preview JavaScript from accessing IDE device credentials
(`localStorage`) while keeping multi-file static sites working.

## Change

- `BrowsePreview` iframe: `sandbox="allow-scripts"` (drop `allow-same-origin`).
- Preview responses: `Content-Security-Policy: frame-ancestors 'self'`.

## Acceptance criteria

- [x] Preview scripts cannot read parent `localStorage`.
- [x] Relative CSS/JS/images load on loopback, where the server auth bypass
  applies. LAN subresource authentication remains a follow-up because browsers
  do not copy query credentials from the entry URL to relative requests.
- [x] Documented in known-issues as mitigated, with residual risks recorded.

## Out of scope

- Separate preview origin / short-lived preview tokens.
- Blocking third-party network egress from preview scripts.
