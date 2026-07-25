# Project Status — Local Agent Interface

> Updated: 2026-07-24. Task detail lives in `docs/plans/`; deferred gaps in
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
- [~] **ACP core modularization** — callback/events/MCP/diagnostics modules
  extracted (`S-ACP-MOD-CALLBACKS`); actor runtime extracted with terminal-outcome
  watcher (`S-ACP-MOD-ACTOR`); turn, registry, and facade splits remain. See
  `docs/plans/pending-acp-core-modularization-hard.md`.
- [~] **Agent-owned history** — PROBE + fallback complete; migration deferred;
  local history remains for agents without list/load. Q7/Q8 still block browse.
- [~] **Workspace preview** — serving, browse tab, live reload, sandbox, and
  LAN relative-asset auth complete; dev-server proxy, mobile UX, and auto-index
  remain. (Plan files reviewed and pruned 2026-07-22; see
  `docs/reviews/2026-07-22/workspace-preview-lan-auth-compact.md`.)
- [ ] **Phase 2** — concurrent workers, capability negotiation, diagnostics.
- [x] **Profiles over ACP** — config schema/loader, mockagent mode-cap, REST
  CRUD, ACP `set_config_option` send path + `POST /sessions/:id/profile`,
  Settings Profiles tab, and a dynamic chat selector with per-session
  persistence. Profiles now select complete MCP servers; per-tool enumeration
  was superseded because ACP attaches servers, not subsets of server tools.
  (Plan files reviewed and pruned 2026-07-22.)
- [~] **Profile MCP transitions** — active: keep profile switching available,
  while offering ACP-safe new-session, fresh-chat, or instructions-only choices
  when MCP access differs. See
  `docs/plans/other_tasks/active-profile-mcp-transition-hard-high.md`.

## Blocked

- **MCP-over-ACP** — legacy Go SDK did not generate `mcp/message`; Rust SDK
  supports it, but the broker is not wired. Inline MCP remains the active path.

## Recent Changes (2026-07)

- **S-ACP-MOD-ACTOR** — actor startup, SDK connection construction, initialize,
  and session new/load resolution moved to `src/acp/core/actor/mod.rs`; the
  actor→registry back-reference was replaced with a terminal-outcome channel
  consumed by a lifecycle watcher that owns session-failed transitions and
  AgentExited publication. `core.rs` reduced ~724 lines.
- **S-ACP-MOD-CALLBACKS** — inbound callbacks, terminals, MCP attachment,
  ordered event append, and stderr diagnostics moved out of `core.rs` into
  private `src/acp/core/{events,diagnostics,mcp,handlers}` modules.
- **S-PROF-ACP** — profile delivered to the agent over ACP via
  `session/set_config_option` (mode category) on session setup and on
  `POST /api/sessions/:id/profile`; the deprecated `profile` field on
  `/prompt` is removed (breaking wire change). Capability gate falls back to
  prompt injection when the agent lacks the mode option.
- **Port-orphan recovery** — `start` probes the HTTP port before binding and
  names the holding PID; `stop` falls back to a port-listening lookup when no
  live PID file exists, so an orphaned daemon is recoverable via the CLI.
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
