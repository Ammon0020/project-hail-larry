# Project Status — Local Agent Interface

> Last updated: 2026-07-17. Source of truth for task-level status. Keep < 150 lines.
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

- [ ] **Missing workspace user warning** — Don’t auto-remove missing paths from config; show UI/CLI warning instead (temp prune in `daemon.go`).
- [ ] **Editor on mobile** — CodeMirror touch optimization.
- [ ] **Profile mode (Code/Ask/Plan)** — composer selector is a UI placeholder; needs to be sent with the prompt.
- [ ] **MCP store/settings icons** — popout header icons are no-ops.
- [ ] **QR/pair scheme selection** — `app pair`/QR encode the HTTPS URL only; let device pick, or encode both.
- [ ] **Multi-user vs multi-device** — multi-device/single-user decided; multi-user remains future.
- [ ] **ACP futures** — sub-workers, session fork/resume/close, elicitation, NES, audio, ACP-inspector.
- [ ] **Phase 2 (Multi-Agent)** — multiple simultaneous workers, capability negotiation, enhanced diagnostics.
- [ ] **Rust backend port** — Phase 0–2 complete (245 tests). Phase 3 in progress:
  S-MCP/S-ACP-AUTODETECT/S-WORKSPACE/S-PAIRING complete (268 tests); ACP core
  transport/actor and secure filesystem, permission, and terminal handlers are in place;
  lifecycle hardening tests are in place; stream/context/provider stories remain.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`). Inline transport retained. Rust SDK closes this gap. Docs: `acp-spec-compliance.md` §4.10, `known-issues.md`.

## Recent Changes (2026-07)

- **07-17** — Rust port **S-ACP-CORE** started: `src/acp/core.rs` has a
  constructor-wired session registry and a per-session actor using manual
  `async_process` + SDK `ByteStreams`, workspace cwd, owned child teardown, and
  bounded stderr tails. Filesystem, permission, and terminal callbacks now enforce
  workspace containment, bounded output, child cancellation, and non-blocking waits.
  Actor control now preempts prompts for cancel/close, failures mark sessions failed,
  permission cleanup uses local IDs, and callback/terminal resources are bounded.
  Refactor: shared callback dispatch helpers, single `SessionEntry::apply_state`
  (new sessions expose `idle`), single close ack, and small dispatch cleanups.
- **07-17** — Rust port **S-MCP** and **S-ACP-AUTODETECT** complete.
  `src/mcp` provides compatible atomic MCP config persistence, transport capability
  conversion, environment expansion, and bounded health checks. `src/acp` now has a
  thread-safe agent registry and fixed-argv known-agent discovery; provider probing is
  explicitly opt-in, bounded, and redacts diagnostics. Tests: 250 lib + 7 ACP spike.
- **07-17** — Rust port **S-WORKSPACE** complete: `src/workspace`. The concurrent
  registry provides bounded, symlink-safe tree/read/write/raw-path/search access with
  Go-compatible content-hash revisions and preview flags. Tests: 261 lib + 7 ACP spike.
- **07-17** — Rust port **S-PAIRING** complete: `src/pairing`. Pairing has QR +
  mnemonic sessions, hash-only atomic device persistence, constant-time validation,
  bounded verification lockout, sliding credentials, and injected grace actions.
  Secrets are redacted from debug output. Tests: 268 lib + 7 ACP spike.
- **07-16** — Rust port **S-PERMISSIONS** complete: `src/permissions/{mod,sink,manager,tests}.rs`. `Manager` implements `PermissionManager` (request/respond/clear_session/get_pending). Pending prompts use request ID + `oneshot` sender; awaiting a decision never holds the manager lock (sync `Mutex<Inner>` for short critical sections, no `.await` under lock). First-response-wins via map-remove-then-send. Policies: `allow_always`/`allow_session`/`reject_always` keyed by (session, tool, target) for file tools, (session, tool, command) for shell tools (closes the shell-command bypass). Stale sweeper: `tokio::time::interval` task auto-denies after 5min (configurable timeout for tests). Cancellation = future-drop, cleaned up by an RAII `PendingCleanup` guard. Replaces Go's `SetCallback` with a constructor-injected `PermissionSink` (`EventBusPermissionSink` persists+publishes `PermissionRequested` via `EventBus`; `NullSink`/`CapturingSink` for tests). Ephemeral audit log (bounded 10k). 25 permission tests (request/respond, first-response-wins, timeout, invalid decision, allow_always/session/once auto-resolve, session scoping, clear_session, shell-command bypass/same-command, stale sweep + sweeper task, reject_always auto-deny/target-scope/deny-cache-clear, clear_session denies pending, default options, EventBus sink integration). Tests: 25 permissions + prior = 238 lib + 7 spike = 245.
- **07-16** — **Contract differential runner** complete: `tests/contract_runner/` (Rust `cargo test` integration test). Black-box runner boots a backend binary (Go or Rust) as a subprocess, replays HTTP/WS request sequences from the golden fixtures, applies the same redactions, and compares. 68 tests (45 REST + 2 WS + 3 DTO + redactor/compare unit tests), all passing against Go backend. CLI tests intentionally excluded — CLI is a thin client over the API, its formatting is presentation not contract. `CONTRACT_BACKEND=go|rust`, `CONTRACT_BINARY`, `CONTRACT_KEEP_STATE` env vars. `make test-contract` target. Docs: `tests/contract/README.md` § Rust black-box differential runner.
- **07-16** — Rust port **S-FSWATCH** complete: `src/fswatch/{mod,tests}.rs`. On-disk change detection via `notify` + `notify-debouncer-full` (preserves `EventKind` for Create-dir handling). Three std threads (debouncer callback, worker owning the `Debouncer`, emit drainer). Recursive per-dir NonRecursive watches (skips `.git`/`node_modules`/…/hidden), app-write suppression (2s TTL) + per-path emit throttle (300ms) via bounded `lru::LruCache`, opportunistic 30s cleanup, ignored-component filtering, `Access`/`Other` event filtering (notify noise Go's fsnotify never sees). Emits `FileChangedOnDisk` through a pluggable emit callback (caller wires to `EventBus`). Deps: `notify`, `notify-debouncer-full`, `lru`. Tests: 9 fswatch + prior = 168 lib + 7 spike = 175.
- **07-16** — Rust port **S-SHELL** complete: `src/shell/{mod,tests}.rs`. Workspace-scoped subprocess runner (`tokio::process::Command`), line-by-line stdout/stderr streaming, CWD containment via `pathutil::clean_path`, per-command `CancellationToken` with process-group kill (Unix `setpgid`+`killpg`, Windows `CREATE_NEW_PROCESS_GROUP`), bounded output (1 MiB default), `merge_env`. 20 shell tests (echo, exit codes, CWD, streaming, cancellation, process-group orphan prevention, path traversal, timeout, output cap, env). Deps: `libc`. Tests: 20 shell + prior = 159 lib + 7 spike = 166.
- **07-16** — Rust port **S-UPLOADS** complete: `src/uploads/{mod,tests}.rs` — per-session upload store (`Manager`), v4 UUID opaque IDs, `infer` magic-byte MIME detection (PNG/JPEG/GIF/WebP), 10 MB cap, path-traversal-safe session/upload ID validators, `store`/`get`/`remove_session`/`remove_all`. Deps: `uuid` (v4), `infer`. Tests: 18 uploads + prior = 139 lib + 7 spike = 146.
- **07-15** — Rust port **S-MIGRATE** complete: `src/migrate/{mod,detect,config,validate,error,tests}.rs` + `tests/migrate/fixtures/go-state/`. Atomic/idempotent/restart-safe `config.json`→`config.toml` (versioned `config.json.bak.v1`, dual-state keeps Go readable). Validates event DB (Rust opens Go SQLite), structurally validates devices/conversations/mcp/uploads/tls (semantic load deferred). Tests: 21 migrate + prior = 120 lib + 7 spike = 127. Next: service ports.
- **07-15** — Rust port **S-EVENTS** complete: SQLite WAL store, schema matching Go, EventBus. Tests: 27 events + prior = 99 lib + 7 spike.
- **07-15** — S-INTERFACES, S-CONFIG, S-PATHUTIL, S-ACP-SPIKE, S-CONTRACT, S-ARCH; shared fsutil; daemon start resilience.
- **07-13–07-06** — Chat/MCP/auth/ACP + binary previews; MCP-over-ACP Go SDK gap documented.

## Known Gaps (summary — see `docs/known-issues.md`)

- Go ACP SDK still missing `mcp/message` relay (Rust path unblocked).
- Mobile editor touch; profile mode not wired; pair QR scheme selection.
- Rust port Phase 1 complete (S-MIGRATE/S-UPLOADS/S-SHELL done); Phase 2+ service ports. Pairing/MCP/ACP semantic load deferred past structural validation.
