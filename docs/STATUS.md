# Project Status — Local Agent Interface

> Updated: 2026-07-18. Task detail lives in `docs/plans/`; deferred gaps in
> `docs/known-issues.md`; architecture is `docs/plans/Blueprint.md`.

## What Works

- **Daemon + CLI** — lifecycle, folder management, pairing, devices, logs, and
  systemd/launchd/HKCU service installation; config persistence and TLS.
- **Dual HTTP/HTTPS** — HTTP plus self-signed HTTPS when `tlsEnabled` (default).
- **ACP client** — lifecycle, streaming, tool calls, thoughts/plans, permission
  policies, shell terminal, PKCE auth, and agent autodetection.
- **Pairing + auth** — QR/mnemonic pairing, sliding-TTL device credentials,
  grace-period revocation, and Bearer/WS authentication.
- **WebSocket + event store** — live sync, replay, reconnect/keepalive, and
  SQLite WAL append-only event storage with retention.
- **Files + workspace** — revisions, three-way merge, path/symlink containment,
  on-disk change detection, file mutations, search, and previews.
- **MCP + permissions** — Claude-compatible config, inline transports, settings,
  prompt policies, stale-prompt denial, and audit logging.
- **Frontend** — React/Vite/Tailwind UI, responsive editor/chat/explorer,
  CodeMirror, search, uploads, themes, offline shell, and binary previews.
- **Security** — authenticated routes, WS Origin/CSRF checks, rate/body caps,
  workspace containment, and a 2026-07-07 audit (one deferred finding).
- **Rust port** — Go daemon removed; `cmd/mockagent` remains for tests.

## Active TODO

- [x] Missing-workspace warning, mobile editor, profile mode, and MCP settings.
- [ ] **QR/pair scheme** — choose HTTPS-only, both URLs, or a device picker.
- [ ] **Multi-user** — multi-device/single-user is decided; multi-user is future.
- [ ] **ACP futures** — workers, session lifecycle, elicitation, NES, audio,
  and ACP inspector.
- [~] **Agent-owned history** — PROBE + fallback complete; local history remains
  for agents without list/load. Q7/Q8 still block browse.
- [~] **Workspace preview** — serving, browse tab, live reload, sandbox, and
  LAN relative-asset auth complete; dev-server proxy, mobile UX, and auto-index
  remain. See `docs/plans/complete-workspace-preview-small.md`.
- [ ] **Phase 2** — concurrent workers, capability negotiation, diagnostics.

## Blocked

- **MCP-over-ACP** — legacy Go SDK did not generate `mcp/message`; Rust SDK
  supports it, but the broker is not wired. Inline MCP remains the active path.

## Recent Changes (2026-07)

- **S-HIST-PROBE** — harness matrix and live capability projection; no cold-start.
- **Preview LAN auth** — one-time, workspace-bound entry ticket exchanges for an
  HttpOnly path cookie; native TLS marks it `Secure`; relative assets now load.
- **Preview hardening** — opaque-origin iframe (`allow-scripts` only),
  `frame-ancestors 'self'`, and debounced file-change reload.
- **Mobile + file tree** — touch editor behavior; complete context menu and safe
  delete/rename/mkdir routes with tab remapping.
- **Compaction** — simpler ACP lock/workspace helpers, pairing paths, error
  construction, and session-capability initialization.
- **Rust cutover** — Go daemon removed, contract harness uses Rust, and the
  Rust server/ACP/file stories are complete.
- **Security/runtime** — temp-config poison guard, dual listeners, request caps,
  process-group cleanup, and workspace registration gate.

## Known Gaps

- Reinstall the service if it still points at the legacy `app` binary.
- Pairing QR currently uses HTTP; choose the scheme policy above.
- MCP broker, ignored contract text differences, and black-box slow-client WS
  coverage remain deferred. HTTP/2 header deadline is not available in Hyper.
