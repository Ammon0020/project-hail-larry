# Project Status — Local Agent Interface

> Updated: 2026-07-29. Task detail lives in `docs/plans/`; deferred gaps in
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
- **Rust port** — Go daemon and Go mockagent both removed; mockagent is now a
  Rust binary at `src/bin/mockagent.rs`.

## Active TODO

- [x] Missing-workspace warning, mobile editor, profile mode, and MCP settings.
- [ ] **QR/pair scheme** — choose HTTPS-only, both URLs, or a device picker.
- [ ] **Multi-user** — multi-device/single-user is decided; multi-user is future.
- [ ] **ACP futures** — multi-client gateway, workers, session lifecycle,
  elicitation, NES, audio, and ACP inspector. See
  `docs/plans/pending-multi-client-acp-gateway-med.md`.
- [x] **Git action bar + diff viewer** — workspace git detection, backend
  status/diff/stage/unstage/commit/push/init API (`gix` + git CLI porcelain),
  reusable CodeMirror merge diff viewer, Source Control activity-bar panel,
  and git init. Foundational for the edited-files popup. See
  `docs/plans/git-action-bar/` (all stories done).
- [x] **ACP core modularization** — callbacks, actor runtime, turn state,
  session registry, lifecycle, operations, and the thin client facade are
  extracted (`S-ACP-MOD-CALLBACKS` through `S-ACP-MOD-FACADE`). See
  `docs/plans/complete-acp-core-modularization-hard.md`.
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

- **Git module split** — Git DTOs, repository/history, worktree operations,
  CLI helpers, and tests moved out of `src/git/mod.rs`; the facade is now 34
  lines. See `docs/plans/other_tasks/done-git-module-split-large-high.md`.
- **API module split** — REST handlers, auth, pairing, files, previews, sessions,
  and shared test support moved into focused `src/api/` modules; `mod.rs` now
  owns composition and shared plumbing. See
  `docs/plans/other_tasks/done-api-module-split-large-medium.md`.
- **Git log API (S-GIT-LOG-API)** — `GET /api/workspaces/{id}/git/log?limit=&offset=`
  implemented with `gix::Repository::rev_walk()` for the commit graph, branch
  labels, and ISO 8601 UTC author timestamps. 7 unit tests. See
  `docs/plans/git-history-graph/done-git-log-api-medium.md`.
- **Git discard button** — client-side discard (untracked → delete, tracked →
  restore base via readFile/getGitDiff/saveFile). Backend discard endpoint
  still pending (`pending-git-discard-endpoint-med-high.md`).
- **useBackend hook extraction** — file and session action hooks extracted
  from `useBackend.ts` into `useFileActions.ts` and `useSessionActions.ts`.
- **Permission grant transparency** — durable permission decisions
  (`allow_session`/`allow_always`/`reject_always`/`allow_tool_kind`) now show a
  confirm step with the exact grant scope before the user commits. Tool-kind
  scoping (`AllowToolKind`) added for a conservative allowlist
  (move/edit/read/search, never execute). See
  `docs/plans/other_tasks/done-permission-grant-transparency-med-med.md`.
- **Git integration** — full Source Control surface shipped: `gix`-based
  workspace detection (`GET /git`), status/diff/stage/unstage/commit/push/init
  REST API (authenticated paired-device gate, no permission sink), CodeMirror
  merge diff viewer tab (`@codemirror/merge`), Source Control activity-bar
  panel with stage/unstage/commit/push and a repo-init affordance, and dynamic
  branch display in the status bar (replacing the hardcoded "main"). Contract
  tests cover the read-only git endpoints; init contract test deferred per
  `docs/known-issues.md` (harness isolation).

- **S-ACP-MOD-ACTOR** — actor startup, SDK connection construction, initialize,
  and session new/load resolution moved to `src/acp/core/actor/mod.rs`; the
  actor→registry back-reference was replaced with a terminal-outcome channel
  consumed by a lifecycle watcher that owns session-failed transitions and
  AgentExited publication. `core.rs` reduced ~724 lines.
- **S-ACP-MOD-TURN** — actor commands, prompt/control dispatch, cancellation,
  stop-reason mapping, and prompt/close tests moved to `actor/turn.rs`. Cancelled
  sessions now retain their interrupted state until explicit or grace teardown,
  so a non-cooperative agent is force-closed instead of disabling the watchdog.
- **S-ACP-MOD-REGISTRY** — live/dormant metadata, state transitions, actor
  handles, and merged durable snapshots moved to `core/registry.rs`; registry
  operations return owned data and never expose a lock across async lifecycle work.
- **S-ACP-MOD-FACADE** — `core.rs` is now a 34-line module surface; client,
  lifecycle, and operations own their respective responsibilities. Regressions
  cover stale actor generations, concurrent restore serialization, and session caps.
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
  Rust server/ACP/file stories are complete; Go mockagent replaced by
  `src/bin/mockagent.rs`.
- **Security/runtime** — temp-config poison guard, dual listeners, request caps,
  process-group cleanup, and workspace registration gate.
- **S-GIT-FETCH-PULL** — `POST /git/fetch` and `POST /git/pull` endpoints
  complete the remote sync loop; `status()` now returns real
  `upstream`/`ahead`/`behind` via `git rev-parse @{u}` + `rev-list --count`;
  pull refuses dirty trees (409). GitPanel header has a Fetch button and a
  "Pull from remote" dropdown action. See
  `docs/plans/git-history-graph/done-git-fetch-pull-small.md`.
- **S-GIT-CHECKOUT** — `POST /git/checkout` switches local branches (refuses
  dirty trees with 409); `status()` now returns `branches: Vec<String>` for the
  dropdown. GitPanel header branch name replaced with a dropdown showing all
  local branches with a check on the current one. See
  `docs/plans/git-history-graph/done-git-checkout-small.md`.

## Known Gaps

- Reinstall the service if it still points at the legacy `app` binary.
- Pairing QR currently uses HTTP; choose the scheme policy above.
- MCP broker, ignored contract text differences, and black-box slow-client WS
  coverage remain deferred. HTTP/2 header deadline is not available in Hyper.
