# Security Review — 2026-07-23

Full-codebase security audit of `local_agent`. Findings are written as individual
markdown files named `<slug>,<difficulty>,<urgency>.md`. Files are deleted when
the finding is fixed or verified as a false positive.

## Summary

| Urgency | Open | Fixed | False Positive |
|---------|------|-------|----------------|
| Critical | 1 | 6 | 1 |
| High | 0 | 16 | 1 |
| Medium | 9 | 12 | 0 |
| Low | 16 | 5 | 1 |
| **Total** | **26** | **39** | **3** |

---

## Open findings — Critical

| Finding | Difficulty | File |
|---------|-----------|------|
| [HTTP listener serves full API surface with TLS](http-listener-serves-full-surface-with-tls,easy,critical.md) | easy | `src/app/listen.rs` |

## Open findings — Medium

| Finding | Difficulty | File |
|---------|-----------|------|
| [Cleartext HTTP listener with TLS enabled](cleartext-http-listener-with-tls,easy,medium.md) | easy | `src/app/listen.rs` |
| [No command timeout](no-command-timeout,easy,medium.md) | easy | `src/shell/mod.rs` |
| [Pairing lockout is global — DoS](pairing-lockout-global-dos,trivial,medium.md) | trivial | `src/pairing/mod.rs` |
| [Policy key has no CWD/env validation](policy-key-no-cwd-env,medium,medium.md) | medium | `src/acp/core.rs` |
| [Search has no file count cap](search-no-file-count-cap,easy,medium.md) | easy | `src/search/mod.rs` |
| [setsid escapes process group kill](setsid-escapes-pgroup-kill,easy,medium.md) | easy | `src/shell/mod.rs` |
| [No CSP on SPA shell](spa-no-csp,easy,medium.md) | easy | `web/index.html` |
| [TLS cert generation silent on failure](tls-cert-generation-silent,easy,medium.md) | easy | `src/tls/mod.rs` |
| [Upload has no aggregate size cap](upload-no-aggregate-cap,medium,medium.md) | medium | `src/api/session_extra.rs` |

## Open findings — Low

| Finding | Difficulty | File |
|---------|-----------|------|
| [Absolute paths leaked in error messages](absolute-paths-leaked-in-errors,easy,low.md) | easy | `src/workspace/mod.rs` |
| [autodetect_agents not rate-limited — probe DoS](autodetect-no-rate-limit,medium,low.md) | medium | `src/api/mod.rs` |
| [Cancel is cooperative — agent can ignore](cancel-cooperative-no-kill,medium,low.md) | medium | `src/acp/core.rs` |
| [Device secret uses plain SHA-256 (no salt/HMAC)](device-secret-plain-sha256,medium,low.md) | medium | `src/pairing/mod.rs` |
| [filesync trusts workspace_id](filesync-trusts-workspace-id,easy,low.md) | easy | `src/sync/mod.rs` |
| [Office HTML injected without DOMPurify](office-html-no-sanitize,medium,low.md) | medium | `web/src/components/FileViewer.tsx` |
| [Pairing QR PNG persists to disk](pairing-qr-png-persists,easy,low.md) | easy | `src/pairing/mod.rs` |
| [Peer addr defaults to loopback](peer-addr-defaults-loopback,hard,low.md) | hard | `src/api/mod.rs` |
| [No per-field size limit on provider/agent fields](provider-agent-no-field-size-limit,easy,low.md) | easy | `src/api/providers.rs` |
| [CWD resolution is lexical only](resolve-cwd-lexical-only,medium,low.md) | medium | `src/shell/mod.rs` |
| [Search dead cancellation token](search-dead-cancellation-token,easy,low.md) | easy | `src/search/mod.rs` |
| [TLS 1.2 enabled](tls12-enabled,trivial,low.md) | trivial | `src/tls/mod.rs` |
| [TLS key not zeroized](tls-key-not-zeroized,hard,low.md) | hard | `src/tls/mod.rs` |
| [Upload has no symlink check](upload-no-symlink-check,hard,low.md) | hard | `src/api/session_extra.rs` |
| [Windows process group kill incomplete](windows-pgroup-kill-incomplete,medium,low.md) | medium | `src/shell/mod.rs` |
| [WS credentials in query string](ws-creds-in-query-string,trivial,low.md) | trivial | `web/src/lib/api.ts` |

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

Six batches of fixes were applied during the review, each verified with
`cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt`:

- **Fix Set 1:** cert dir permissions, events DB perms, 0.0.0.0 origin, output byte limit
- **Fix Set 2:** loopback IPv4-mapped IPv6, revocation device-id leak, raw file unbounded read, fswatch symlinks
- **Fix Set 3:** serve_upload headers, global security headers, extract_credential query fallback, pair_rate bucket growth
- **Fix Set 4:** upsert/delete agent loopback gates, terminal env hardening, device/session name validation, iframe sandboxing, MCP PUT size cap, stderr redaction expansion
- **Fix Set 5:** MCP env expansion + config validation + GET redaction, WS TLS gate, revoked device WS disconnect, session_context session-keying, cancel-revocation self-cancellation guard, workspace root symlink rejection
- **Fix Set 6:** create_terminal approval gate, session count cap, HTTP→HTTPS redirect (replaces WS block), event replay verified by-design, WS connection limit, hub fail-closed, raw/preview CSP sandbox, IPv6 /64 rate limiting, frontend sessionStorage + preview-session URLs, workspace root TOCTOU-safe open

All 443 tests pass; clippy and fmt are clean.

## False positives (3)

- **event-replay-unscoped** (critical): Single-user trust model — all paired devices share all state by design. Scoping replay while leaving live fan-out unscoped would be pointless.
- **remote-workspace-arbitrary-path** (critical): `register_workspace` already gates on `allow_remote_workspace_registration` which defaults to `false`.
- **fs-op-toctou-symlink** (medium): Theoretical TOCTOU race requiring concurrent filesystem access from a trusted entity that already has full workspace access. Existing `resolve_symlink` provides adequate defense-in-depth.
