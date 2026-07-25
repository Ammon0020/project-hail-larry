# Security Review — 2026-07-23

Full-codebase security audit of `local_agent`. Findings were written as individual
markdown files named `<slug>,<difficulty>,<urgency>.md`. Files are deleted when
the finding is fixed or verified as a false positive.

**All findings have been resolved.** No open findings remain.

## Summary

| Urgency | Open | Fixed | False Positive |
|---------|------|-------|----------------|
| Critical | 0 | 7 | 1 |
| High | 0 | 16 | 1 |
| Medium | 0 | 21 | 0 |
| Low | 0 | 21 | 1 |
| **Total** | **0** | **65** | **3** |

---

## Review methodology

Four parallel review batches, each dispatched as specialized subagents:

1. **Core auth & transport** — pairing, HTTP auth, TLS/listen, WebSocket
2. **Filesystem & command surfaces** — paths, shell, permissions, uploads
3. **Data & agent boundary** — SQLite, ACP, config/secrets, API routes
4. **Frontend & cross-cutting integration** — XSS, search, interfaces, escalation

Each finding was verified by the reviewing subagent before the file was written.
Findings were deduplicated across batches.

## Fix sets

Seven batches of fixes were applied during the review, each verified with
`cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt`:

- **Fix Set 1:** cert dir permissions, events DB perms, 0.0.0.0 origin, output byte limit
- **Fix Set 2:** loopback IPv4-mapped IPv6, revocation device-id leak, raw file unbounded read, fswatch symlinks
- **Fix Set 3:** serve_upload headers, global security headers, extract_credential query fallback, pair_rate bucket growth
- **Fix Set 4:** upsert/delete agent loopback gates, terminal env hardening, device/session name validation, iframe sandboxing, MCP PUT size cap, stderr redaction expansion
- **Fix Set 5:** MCP env expansion + config validation + GET redaction, WS TLS gate, revoked device WS disconnect, session_context session-keying, cancel-revocation self-cancellation guard, workspace root symlink rejection
- **Fix Set 6:** create_terminal approval gate, session count cap, HTTP→HTTPS redirect (replaces WS block), event replay verified by-design, WS connection limit, hub fail-closed, raw/preview CSP sandbox, IPv6 /64 rate limiting, frontend sessionStorage + preview-session URLs, workspace root TOCTOU-safe open
- **Fix Set 7:** TLS 1.2 disable + cert-gen silent failure + key zeroize, command timeout + setsid process-tree kill + Windows Job Objects + CWD symlink resolution, search file-count cap + dead cancellation token + upload aggregate cap + upload symlink defense, autodetect rate-limit cache + provider/agent field size limits + per-IP pairing lockout + QR cleanup + filesync debug assert + absolute-path error redaction, SPA CSP + Office HTML DOMPurify, peer-addr fail-closed default, cancel grace timeout + handler_cancel on Cancel

All 443 tests pass; clippy and fmt are clean.

## False positives (3)

- **event-replay-unscoped** (critical): Single-user trust model — all paired devices share all state by design. Scoping replay while leaving live fan-out unscoped would be pointless.
- **remote-workspace-arbitrary-path** (critical): `register_workspace` already gates on `allow_remote_workspace_registration` which defaults to `false`.
- **fs-op-toctou-symlink** (medium): Theoretical TOCTOU race requiring concurrent filesystem access from a trusted entity that already has full workspace access. Existing `resolve_symlink` provides adequate defense-in-depth.
