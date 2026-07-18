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
  REST mutations: delete file/empty dir, rename (no overwrite), mkdir (parents).
- **MCP** — Claude-compatible `~/.local-agent/mcp.json`; inline stdio/http/sse transports
  to the agent (capability-filtered); health status endpoint; settings editor + composer
  toggle popout.
- **Frontend** — React 19 + Vite 8 + Tailwind v4 + shadcn/ui. Desktop/mobile layouts, chat
  (streaming, markdown, tool/plan cards, rename/delete/rebind/export, autoscroll),
  CodeMirror 6 editor, file tree (full context menu), search, settings-as-tab,
  image upload, SPA offline shell (service worker), light/dark/system theme.
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
- [x] **Editor on mobile** — CodeMirror touch config (`EditorPane.tsx`).
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
- [ ] **Agent-owned session history** — stories drafted (not impl); index:
  `docs/plans/pending-acp-agent-session-history-med.md` →
  `docs/plans/acp-session-history/` (PROBE→BROWSE→OPEN→SYNC; FALLBACK; MIGRATE).
  Blocked on epic Decision Needed Q1–Q8.
- [~] **Workspace static preview** — serve + browse tab + live-reload done;
  dev-server proxy / mobile UX / auto-index still open.
  `docs/plans/complete-workspace-preview-small.md`.
- [ ] **Phase 2 (Multi-Agent)** — multiple simultaneous workers, capability negotiation,
  enhanced diagnostics.
- [x] **Rust backend port** — Cutover complete: Go `cmd/app` + `internal/`
  deleted; `cmd/mockagent` kept for tests. Stories closed. Daemon on :7337.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the
  `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`). Inline transport retained.
  Rust SDK closes this gap. Docs: `acp-spec-compliance.md` §4.10, `known-issues.md`.

## Recent Changes (2026-07)

- **07-18** — **Browse preview live reload**: `FileWritten` /
  `FileChangedOnDisk` → debounced iframe remount via shared `backend.events`.
- **07-18** — **Editor on mobile**: CodeMirror touch config — scaled lineHeight,
  no DnD/fold gutter, scrollMargins + visualViewport keep-in-view.
- **07-18** — **File-tree context menu**: row + empty-area right-click/
  long-press — Open, Preview (html), Copy, Rename, Delete, New File/Folder
  (empty area → workspace root); delete/rename/mkdir APIs + tab remap.
- **07-18** — Workspace file mutations REST: `DELETE .../file`,
  `POST .../rename`, `POST .../mkdir` (path-safe; empty-dir delete; no
  overwrite; mkdir creates parents). Frontend file-tree context menu.
- **07-18** — Compact-code (safe): ACP lock helpers + `resolve_workspace` /
  `map_live_session`; API `ApiResponseError::new` + shared pair verify;
  pairing `request_pending`; drop dead `ShellError::Pipe`; Hub ctor merge.
- **07-18** — **Browse preview MVP**: `GET /preview/{id}/{*path}` (Rust),
  `BrowsePreview` iframe tab, file-tree "Open Preview" for `.html`/`.htm`.
  Serve story complete; live-reload added same day.
- **07-18** — **Go backend deleted** (`cmd/app`, `internal/`, go-fixtures).
  Build Rust-only; `CONTRACT_BACKEND=go` panics clearly; `go.mod` = mockagent.
- **07-18** — **Config poison fix**: `Config::save` refuses temp `data_dir` when
  state dir is not under temp; daemon tests set `LOCAL_AGENT_STATE_DIR`;
  contract harness uses fake `HOME` under state dir. See known-issues.
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
- **07-18** — Agent-owned session history: **stories drafted** (PROBE,
  BROWSE, OPEN, SYNC, FALLBACK, MIGRATE). Epic status Stories drafted;
  Q1–Q8 still Decision Needed. No product code.
- **07-18** — Rust live chat streaming (EventBus→Hub); MCP on session/new.
- **07-16–07-06** — S-PERMISSIONS through S-ARCH; chat/MCP/auth/ACP +
  binary previews; MCP-over-ACP Go SDK gap.

## Known Gaps (summary — see `docs/known-issues.md`)

- Re-run `local_agent install-service` if units still point at legacy `app`.
- Rust MCP-over-ACP broker unused (`unstable_mcp_over_acp`); inline MCP via
  session/new is the path today.
- Pair QR scheme product choice (currently HTTP).
- Contract ignored: MCP JSON parse-error text; autodetect golden. Slow-client
  WS only unit-tested (not black-box).
- Hyper HTTP/2 header deadline unavailable; body timing starts at handler.
- **2026-07-18:** Rust cutover complete — Go `cmd/app` + `internal/` deleted;
  only `cmd/mockagent` remains. All rust-port stories closed.
