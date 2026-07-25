# Security Review — 2026-07-23

Full-codebase security audit of `local_agent`. Findings are written as individual
markdown files named `<slug>,<difficulty>,<urgency>.md`. Files are deleted when
the finding is fixed.

## Summary

| Urgency | Open | Fixed |
|---------|------|-------|
| Critical | 4 | 2 |
| High | 2 | 14 |
| Medium | 12 | 9 |
| Low | 20 | 5 |
| **Total** | **38** | **30** |

---

## Open findings — Critical

| Finding | Difficulty | File |
|---------|-----------|------|
| [create_terminal executes with no permission gate](create-terminal-no-approval,trivial,critical.md) | trivial | `src/acp/core.rs` |
| [HTTP listener serves full API surface with TLS](http-listener-serves-full-surface-with-tls,easy,critical.md) | easy | `src/app/listen.rs` |
| [Event replay unscoped — any device reads all events](event-replay-unscoped,critical,critical.md) | critical | `src/sync/mod.rs` |
| [raw/preview same-origin XSS via iframe](raw-preview-same-origin-xss,easy,critical.md) | easy | `src/api/mod.rs` |
| [Remote workspace arbitrary path registration](remote-workspace-arbitrary-path,trivial,critical.md) | trivial | `src/api/mod.rs` |

## Open findings — High

| Finding | Difficulty | File |
|---------|-----------|------|
| [Device credential stored in localStorage (XSS-exfiltrable)](device-credential-localstorage,easy,high.md) | easy | `web/src/lib/api.ts` |
| [Device secret leaked via query-param URLs for raw endpoint](raw-url-secret-referer-leak,easy,high.md) | easy | `web/src/lib/api.ts` |
| [Workspace root TOCTOU symlink](workspace-root-toctou-symlink,medium,high.md) | medium | `src/workspace/mod.rs` |

## Open findings — Medium

| Finding | Difficulty | File |
|---------|-----------|------|
| [Unbounded ACP session creation — process DoS](acp-session-unbounded,easy,medium.md) | easy | `src/acp/core.rs` |
| [Cleartext HTTP listener with TLS enabled](cleartext-http-listener-with-tls,easy,medium.md) | easy | `src/app/listen.rs` |
| [FS op TOCTOU symlink race](fs-op-toctou-symlink,hard,medium.md) | hard | `src/workspace/mod.rs` |
| [Hub fails open when no auth checker registered](hub-fails-open-no-checker,easy,medium.md) | easy | `src/sync/mod.rs` |
| [No command timeout](no-command-timeout,easy,medium.md) | easy | `src/shell/mod.rs` |
| [Pairing lockout is global — DoS](pairing-lockout-global-dos,trivial,medium.md) | trivial | `src/pairing/mod.rs` |
| [Pair rate IPv6 /64 rotation bypass](pair-rate-ipv6-rotation,easy,medium.md) | easy | `src/api/mod.rs` |
| [Policy key has no CWD/env validation](policy-key-no-cwd-env,medium,medium.md) | medium | `src/acp/core.rs` |
| [Search has no file count cap](search-no-file-count-cap,easy,medium.md) | easy | `src/search/mod.rs` |
| [setsid escapes process group kill](setsid-escapes-pgroup-kill,easy,medium.md) | easy | `src/shell/mod.rs` |
| [No CSP on SPA shell](spa-no-csp,easy,medium.md) | easy | `web/index.html` |
| [TLS cert generation silent on failure](tls-cert-generation-silent,easy,medium.md) | easy | `src/tls/mod.rs` |
| [Upload has no aggregate size cap](upload-no-aggregate-cap,medium,medium.md) | medium | `src/api/session_extra.rs` |
| [WS has no connection limit](ws-no-connection-limit,easy,medium.md) | easy | `src/sync/mod.rs` |

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

Five batches of fixes were applied during the review, each verified with
`cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt`:

- **Fix Set 1:** cert dir permissions, events DB perms, 0.0.0.0 origin, output byte limit
- **Fix Set 2:** loopback IPv4-mapped IPv6, revocation device-id leak, raw file unbounded read, fswatch symlinks
- **Fix Set 3:** serve_upload headers, global security headers, extract_credential query fallback, pair_rate bucket growth
- **Fix Set 4:** upsert/delete agent loopback gates, terminal env hardening, device/session name validation, iframe sandboxing, MCP PUT size cap, stderr redaction expansion
- **Fix Set 5:** MCP env expansion + config validation + GET redaction, WS TLS gate, revoked device WS disconnect, session_context session-keying, cancel-revocation self-cancellation guard, workspace root symlink rejection

All 443 tests pass; clippy and fmt are clean.
