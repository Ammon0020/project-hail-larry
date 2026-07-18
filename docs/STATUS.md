# Project Status — Local Agent Interface

> Last updated: 2026-07-18. Source of truth for task-level status. Keep < 150 lines.
> Architecture: `docs/plans/Blueprint.md`. Work streams: `docs/plans/execution-plan.md`.
> Deferred findings/gaps: `docs/known-issues.md`. Open decisions: `docs/plans/OpenItems.md`.
> This file is a status snapshot — full change detail lives in git history and the plans.

## What Works

- **Daemon + CLI** — start/stop/status/add-folder/remove-folder/list-folders/pair/devices/revoke/logs; `install-service`/`uninstall-service` (systemd/launchd/HKCU). TLS + config persistence.
- **Dual HTTP/HTTPS** — both listeners run when `tlsEnabled` (default); pick scheme by URL (`:7337` HTTP, `:7338` HTTPS). Self-signed cert auto-generated.
- **ACP client** — full protocol via `coder/acp-go-sdk`: session lifecycle, streaming, enriched tool calls, thoughts/plans, permission policies, terminal wired to shell executor, agent auth (PKCE), autodetect. Verified E2E.
- **Pairing + auth** — QR + mnemonic passcode, device credentials (sliding-TTL expiry), grace-period revocation (any device can cancel), remote auth via Bearer/WS query params.
- **WebSocket sync** — real-time broadcast, reconnection sync (exp. backoff + jitter, reconnect on online/foreground), keepalive, loopback auth bypass.
- **Permissions** — request/response, `allow_always`/`allow_session` policies, stale-prompt auto-deny (5min + 60s sweep), audit log.
- **Event store** — SQLite (WAL, append-only) with retention pruning; query/replay.
- **File sync** — revision tracking (48-bit content hash), three-way merge, per-file locking, LRU cache, live on-disk change detection (`internal/fswatch`).
- **MCP** — Claude-compatible `~/.local-agent/mcp.json`; inline stdio/http/sse transports to the agent (capability-filtered); health status endpoint; settings editor + composer toggle popout.
- **Frontend** — React 19 + Vite 8 + Tailwind v4 + shadcn/ui. Desktop/mobile layouts, chat (streaming, markdown, tool/plan cards, rename/delete/rebind/export, autoscroll), CodeMirror 6 editor, file tree, search, settings-as-tab, image upload, SPA offline shell (service worker), light/dark/system theme.
- **Binary file previews** — `GET /api/workspaces/{id}/raw` streams raw bytes with Content-Type for browser-native rendering. FileViewer dispatches by extension: images (`<img>`), PDF (`<iframe>`), video/audio (HTML5 players), DOCX (mammoth.js → HTML), STL (Three.js + STLLoader orbit viewer).
- **Security** — auth middleware on all routes, WS Origin/CSRF checks, path-traversal + symlink containment, rate limiting, request/size caps. Full audit 2026-07-07 (9/11 fixed; 1 remaining deferred in known-issues).

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
- [ ] **Multi-user vs multi-device** — multi-device/single-user decided; multi-user remains future.
- [ ] **ACP futures** — sub-workers, session fork/resume/close, elicitation, NES, audio, ACP-inspector.
- [ ] **Phase 2 (Multi-Agent)** — multiple simultaneous workers, capability negotiation, enhanced diagnostics.
- [ ] **Rust backend port** — MCP `session/new` + startup agent autodetect done.
  Next: `session/load` + `acpSessionId`; 50 MiB write body; story AC check-off.
  Daemon on :7337 for UI.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`). Inline transport retained. Rust SDK closes this gap. Docs: `acp-spec-compliance.md` §4.10, `known-issues.md`.

## Recent Changes (2026-07)

- **07-18** — Rust **MCP → session/new**: `ClientDeps.mcp_config_path`; after
  Initialize, capability-filtered servers attached via `.mcp_servers(...)`.
  Malformed mcp.json warns and continues empty (Go parity).
- **07-18** — Rust **fswatch `note_app_write`** via workspace `set_on_write`;
  MCP Settings icon → Settings MCP tab; Go missing-workspace retain (ignore if
  Rust-only focus).
- **07-18** — Go **missing-workspace** parity: `available`/`error`; no prune.
- **07-17** — Rust **fswatch + missing-workspace**: Daemon `Option<Watcher>`;
  emit → EventBus; `WorkspaceInfo.available`/`error`; CLI UNAVAILABLE.
- **07-17** — Rust **S-BUILD** polish: `build.rs` fails if `web/dist/index.html`
  missing; `[profile.release] strip = true`; `docs/development/building.md`
  (`cc`/bundled SQLite); build.sh/ps1 epilogue → `local_agent start`. Deferred:
  native Win/macOS release+SPA smoke CI.
- **07-17** — Rust **S-ACP-CONTEXT** lazy restore: `load_conversations` loads
  metadata without actors; `list_sessions` merges store+live; prompt/cancel/
  providers/rebind spawn actors reusing id/name/agent/model/workspace (EventBus
  history kept). Close of dormant deletes metadata only.
- **07-17** — Rust **contract black-box**: harness seeds `config.toml` (+ JSON
  for Go), PATH/HOME neutralize autodetect, log dir follows
  `LOCAL_AGENT_STATE_DIR`. API parity: Json rejection → JSON 400, not-found
  messages with ids, pairing passcode/token errors, agent PATH warning, raw
  MIME by extension. Remaining: MCP JSON parse-error text (ignored).
- **07-17** — Rust **HTTP/service parity**: HTTP/HTTPS apply header (5s),
  request-body (30s), handler/response (60s), and idle (120s) deadlines.
  `install-service`/`uninstall-service` now use systemd user units, launchd
  LaunchAgents, or the Windows HKCU Run key. Hyper's HTTP/2 header deadline
  remains unavailable; body timing begins at handler entry and a timed-out
  started response closes its stream.
- **07-17** — Rust **pending-actions REST**: GET /api/pending-actions,
  POST cancel-revocation / cancel-registration; revoke/register use grace
  period (202). Pairing list/cancel APIs. Gaps: daemon workspace registrar
  wiring (timer fire), HTTP timeouts, contract black-box.
- **07-17** — Rust **S-SERVER REST completion**: providers (GET/PUT/DELETE,
  Unsupported→501), MCP (GET/PUT/PATCH/status), session export/context/uploads,
  patch rebind/switch_model, prompt profile+attachments. AppState wires mcp path
  + uploads mutex; daemon passes both. Gaps: raw MIME/range parity.
- **07-17** — Rust **S-DAEMON / S-CLI** foundation: daemon composition, PID
  status/stop, dual HTTP/HTTPS rustls listeners, clap tree. install-service stub.
- **07-17** — Rust **S-ACP-CONTEXT**: conversations.json, prompt context,
  export/transfer, idle rebind. Deferred lazy actor restore → done above.
- **07-17** — Rust **S-ACP-PROVIDERS** + **S-SERVER UI-smoke** + **S-SYNC** +
  **S-ACP-STREAM/CORE** (see git history). Provider REST was deferred → done above.
- **07-17** — Focused Rust review cleanup; **S-MCP** / **S-PAIRING** /
  **S-WORKSPACE** / **S-ACP-AUTODETECT** complete.
- **07-16–07-15** — S-PERMISSIONS, S-FSWATCH, S-SHELL, S-UPLOADS, S-MIGRATE,
  S-EVENTS, S-INTERFACES/CONFIG/PATHUTIL/ARCH.
- **07-13–07-06** — Chat/MCP/auth/ACP + binary previews; MCP-over-ACP Go SDK gap.

## Known Gaps (summary — see `docs/known-issues.md`)

- Rust: no ACP `session/load` yet (restart always `session/new`; need
  durable `acpSessionId`).
- Rust MCP-over-ACP broker unused (`unstable_mcp_over_acp`); inline MCP via
  session/new is the path today. Go SDK still missing `mcp/message`.
- Pair QR scheme product choice (currently HTTP); mobile editor touch.
- Contract: MCP JSON parse-error text ignored; autodetect golden ignored.
- Global 10 MiB body cap vs Go 50 MiB file-write exception.
- Story AC checkboxes largely stale (implementation ahead of docs).
- S-BUILD native Win/macOS release+SPA smoke CI deferred.
- Hyper HTTP/2 header deadline unavailable; body timing starts at handler.
