# Project Status — Local Agent Interface

> Last updated: 2026-07-18. Source of truth for task-level status. Keep < 150 lines.
> Architecture: `docs/plans/Blueprint.md`. Work streams: `docs/plans/execution-plan.md`.
> Deferred findings/gaps: `docs/known-issues.md`. Open decisions:
> `docs/plans/OpenItems.md`.
> This file is a status snapshot — full change detail lives in git history and the plans.

## What Works

- **Daemon + CLI** —
  start/stop/status/add-folder/remove-folder/list-folders/pair/devices/revoke/logs;
  `install-service`/`uninstall-service` (systemd/launchd/HKCU). TLS + config persistence.
- **Dual HTTP/HTTPS** — both listeners run when `tlsEnabled` (default); pick scheme by URL
  (`:7337` HTTP, `:7338` HTTPS). Self-signed cert auto-generated.
- **ACP client** — full protocol via `coder/acp-go-sdk`: session lifecycle, streaming,
  enriched tool calls, thoughts/plans, permission policies, terminal wired to shell
  executor, agent auth (PKCE), autodetect. Verified E2E.
- **Pairing + auth** — QR + mnemonic passcode, device credentials (sliding-TTL expiry),
  grace-period revocation (any device can cancel), remote auth via Bearer/WS query params.
- **WebSocket sync** — real-time broadcast, reconnection sync (exp. backoff + jitter,
  reconnect on online/foreground), keepalive, loopback auth bypass.
- **Permissions** — request/response, `allow_always`/`allow_session` policies,
  stale-prompt auto-deny (5min + 60s sweep), audit log.
- **Event store** — SQLite (WAL, append-only) with retention pruning; query/replay.
- **File sync** — revision tracking (48-bit content hash), three-way merge, per-file
  locking, LRU cache, live on-disk change detection (`internal/fswatch`).
- **MCP** — Claude-compatible `~/.local-agent/mcp.json`; inline stdio/http/sse transports
  to the agent (capability-filtered); health status endpoint; settings editor + composer
  toggle popout.
- **Frontend** — React 19 + Vite 8 + Tailwind v4 + shadcn/ui. Desktop/mobile layouts, chat
  (streaming, markdown, tool/plan cards, rename/delete/rebind/export, autoscroll),
  CodeMirror 6 editor, file tree, search, settings-as-tab, image upload, SPA offline shell
  (service worker), light/dark/system theme.
- **Binary file previews** — `GET /api/workspaces/{id}/raw` streams raw bytes with
  Content-Type for browser-native rendering. FileViewer dispatches by extension: images
  (`<img>`), PDF (`<iframe>`), video/audio (HTML5 players), DOCX (mammoth.js → HTML), STL
  (Three.js + STLLoader orbit viewer).
- **Security** — auth middleware on all routes, WS Origin/CSRF checks, path-traversal +
  symlink containment, rate limiting, request/size caps. Full audit 2026-07-07 (9/11
  fixed; 1 remaining deferred in known-issues).

## Active TODO

- [x] **Missing workspace user warning** — Go+Rust: keep path in config; list/CLI
  show `available:false` + error; no auto-prune on daemon load.
- [ ] **Editor on mobile** — CodeMirror touch optimization.
- [x] **Profile mode (Code/Ask/Plan)** — composer sends `profile`; Rust REST
  sets session profile; context pipeline injects instructions.
- [x] **MCP store/settings icons** — Settings opens MCP section; store disabled
  (coming soon).
- [ ] **QR/pair scheme selection** — pair QR currently encodes **HTTP** (not
  HTTPS-only); product choice: HTTPS when TLS on, both URLs, or device picker.
- [ ] **Multi-user vs multi-device** — multi-device/single-user decided; multi-user
  remains future.
- [ ] **ACP futures** — sub-workers, session fork/resume/close, elicitation, NES, audio,
  ACP-inspector.
- [ ] **Agent-owned session history** — epic drafted (needs stories): browse/
  resume chats via agent `session/list`+`load` by `cwd`; thin multi-UI index.
  `docs/plans/pending-acp-agent-session-history-med.md`.
- [ ] **Workspace static preview** — epic drafted: render a multi-file static
  site from the workspace in a preview tab (new `/preview/{id}/*` route +
  iframe). Story `pending-browse-preview-small`.
  `docs/plans/pending-workspace-preview-small.md`.
- [ ] **Phase 2 (Multi-Agent)** — multiple simultaneous workers, capability negotiation,
  enhanced diagnostics.
- [ ] **Rust backend port** — Near cutover: story AC check-off done for
  S-CONTRACT / S-ACP-CORE / S-FILES / S-SERVER / S-ACP-SPIKE / S-BUILD.
  Next: delete Go tree after smoke. Go behind `BUILD_GO=1` /
  `CONTRACT_BACKEND=go`. Daemon on :7337 for UI.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the
  `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`). Inline transport retained.
  Rust SDK closes this gap. Docs: `acp-spec-compliance.md` §4.10, `known-issues.md`.

## Recent Changes (2026-07)

- **07-18** — **S-SERVER closed**: routes/auth/rate-limit/TLS/dual listeners/
  SPA embed/WS hub/body caps/Origin; `api`+`app` tests green; S-CONTRACT
  parity. Plan → `complete-S-SERVER-http-api-med.md`.
- **07-18** — **S-ACP-SPIKE real-agent E2E**: opt-in `ACP_E2E_AGENT` test;
  verified with `codex` (`codex-acp`). CI skips when unset. Spike → complete.
- **07-18** — **S-CONTRACT closed**: CI `contract` job (Linux) runs feature-
  gated suite; WS `?after=` replay, live broadcast, auth-rejection (LAN dial);
  slow-client documented (unit-tested in sync). Plans → complete.
- **07-18** — ACP agent **process-group kill**: shared `src/procutil/`;
  Unix `setpgid`+group SIGKILL on actor shutdown; Windows child-only.
  S-ACP-CORE → complete (descendant reap tests).
- **07-18** — S-FILES closed: three-way merge formally **frontend-owned**
  (`StaleRevision` + base; `@codemirror/merge`). Story → complete.
- **07-18** — Rust **workspace register gate**: no loopback bypass when
  `allowRemoteWorkspaceRegistration` is false (403, Go parity); contract
  `rest_workspaces_register_remote_disabled` fixed.
- **07-18** — Rust **cutover batch**: ACP `session/load` + durable
  `acpSessionId` (`StoredSession`); 50 MiB file-write body; `build.sh`/
  `build.ps1`/`Makefile` Rust-primary (`BUILD_GO=1` for legacy); contract
  harness defaults to `CONTRACT_BACKEND=rust`.
- **07-18** — Epic draft: **agent-owned ACP session history** (list/load by
  `cwd`, cross-editor resume, thin sync). Needs flesh-out — no stories yet.
  `docs/plans/pending-acp-agent-session-history-med.md`.
- **07-18** — Rust **live chat streaming**: EventBus `LiveFanout` → Hub
  broadcast (Go Append→Broadcast). UI `/ws` omits `?after=`; without this
  bridge stream updates stayed in SQLite until refresh/prompt end.
- **07-18** — Rust **MCP → session/new**: `ClientDeps.mcp_config_path`; after
  Initialize, capability-filtered servers attached via `.mcp_servers(...)`.
  Malformed mcp.json warns and continues empty (Go parity).
- **07-17** — Rust **fswatch + missing-workspace**; **S-BUILD** polish;
  S-ACP-CONTEXT lazy restore; contract black-box harness; HTTP/service
  deadlines; pending-actions REST; S-DAEMON/S-CLI (see git).
- **07-16–07-06** — S-PERMISSIONS through S-ARCH; chat/MCP/auth/ACP +
  binary previews; MCP-over-ACP Go SDK gap.

## Known Gaps (summary — see `docs/known-issues.md`)

- Go tree still in-repo (optional `BUILD_GO=1`); delete after smoke + contract
  confidence. Re-run `local_agent install-service` if units still point at `app`.
- Rust MCP-over-ACP broker unused (`unstable_mcp_over_acp`); inline MCP via
  session/new is the path today. Go SDK still missing `mcp/message`.
- Pair QR scheme product choice (currently HTTP); mobile editor touch.
- Contract ignored: MCP JSON parse-error text; autodetect golden. Slow-client
  WS only unit-tested (not black-box).
- S-BUILD native Win/macOS release+SPA smoke CI shipped (see complete plans).
- Hyper HTTP/2 header deadline unavailable; body timing starts at handler.
- **2026-07-18:** rust-port stories closed: S-CONTRACT, S-ACP-CORE, S-FILES,
  S-SERVER, S-ACP-SPIKE (real-agent E2E opt-in), S-BUILD. Next: Go tree delete
  after smoke.
