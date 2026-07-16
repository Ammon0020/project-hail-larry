# Project Status — Local Agent Interface

> Last updated: 2026-07-15. Source of truth for task-level status. Keep < 150 lines.
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
- **Binary file previews** — `GET /api/workspaces/{id}/raw` streams raw bytes with Content-Type for browser-native rendering. FileViewer dispatches by extension: images (`<img>`), PDF (`<iframe>`), video/audio (HTML5 players), DOCX (mammoth.js → HTML), STL (Three.js + STLLoader orbit viewer). Three.js/mammoth are dynamically imported (code-split) so only users who open those file types pay the bundle cost.
- **Security** — auth middleware on all routes, WS Origin/CSRF checks, path-traversal + symlink containment, rate limiting, request/size caps. Full audit 2026-07-07 (9/11 fixed; 1 remaining deferred in known-issues).

## Active TODO

- [ ] **Editor on mobile** — CodeMirror touch optimization.
- [ ] **Profile mode (Code/Ask/Plan)** — composer selector is a UI placeholder; needs to be sent with the prompt.
- [ ] **MCP store/settings icons** — popout header icons are no-ops (settings icon → MCP settings; store icon → future marketplace).
- [ ] **QR/pair scheme selection** — `app pair`/QR encode the HTTPS URL only; let device pick, or encode both.
- [ ] **Multi-user vs multi-device** — decided multi-device/single-user; multi-user/team collaboration remains future.
- [ ] **ACP futures** — sub-workers, session fork/resume/close, elicitation, NES, audio, ACP-inspector (see `acp-spec-compliance.md`).
- [ ] **Phase 2 (Multi-Agent)** — multiple simultaneous workers, capability negotiation, enhanced diagnostics.
- [ ] **Rust backend port** — Phase 0 done (S-ARCH/S-CONTRACT/S-ACP-SPIKE). Phase 1: S-PATHUTIL done; S-CONFIG done. Shared **`src/fsutil`** now owns home (`dirs`) + durable atomic write (`atomic-write-file`); config/logging use it. Port stories/plans updated to prefer crates over hand-rolls (`lru`, `dashmap` 6.x, `rg` hybrid search, `fsutil` for state files; keep custom pathutil). Next: remaining Phase 1 stories.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`) and it can't be wired via the stock `ClientSideConnection`. Inline transport retained. Blocker + drop-in design: `acp-spec-compliance.md` §4.10 and `known-issues.md`.

## Recent Changes (2026-07)

- **07-15** — Shared fsutil (Rust + Go) + library guidance: Rust `src/fsutil` (`dirs` home_dir + `atomic-write-file` durable write); config/logging wired; deps `dirs` 6.0.0 / `atomic-write-file` 0.3.0. Go extracted `internal/fsutil.WriteFileAtomic` from `mcp` — callers config/MCP/ACP store/server MCP PUT. Port stories updated (`rg` hybrid search, `lru`/`dashmap`, no full BIP-39, keep custom pathutil). `cargo fmt`/`clippy -D warnings`/`test` clean; `go test` fsutil/mcp/config/acp/server pass.
- **07-15** — Rust port S-CONFIG complete: `src/config/{error,model,store,tests}.rs` ports `internal/config/config.go` to TOML-on-disk config persistence. `Config`/`AgentInfo`/`AgentModel` use `#[serde(rename_all = "camelCase")]` so TOML field names match Go's JSON tags exactly (Rust reads a JSON→TOML-migrated file with the same keys; S-MIGRATE will provide the atomic migration). On-disk format is `config.toml` (replaces Go `config.json`). Atomic `save`: temp file in same dir → `fsync` → `chmod 0600` → `rename` → best-effort parent `fsync` (mirrors `mcp.WriteFileAtomic`). `load` returns defaults on missing file, parses the raw `toml::Table` first to detect key presence for secure-by-default `tlsEnabled` and 5-minute `revocationGracePeriodSeconds` legacy defaulting (explicit `false`/`0` respected), and fills missing scalars from `default_or_error`. `LOCAL_AGENT_STATE_DIR` env override honored. Unknown forward-compatible TOML keys captured into `#[serde(flatten)] extra: toml::Table` and re-emitted on save (not silently dropped). `ConfigStore` (`Arc<RwLock<Config>>`) provides thread-safe read/write with poison recovery for the daemon's HTTP handlers. `Default` impl logs + falls back to relative `.local-agent` instead of panicking (Rust no-panic policy vs Go `Default()` panic). 13 tests port `config_test.go` plus atomic-write/no-temp-leftover, state-dir override, golden DTO match against `tests/contract/golden/dto/config_default.json`, unknown-field round-trip, `0600` perms (Unix), TLS/grace-period legacy defaulting, workspace add/remove/list, agent upsert/delete, and `ConfigStore` concurrent access. Env-touching tests serialized via a `Mutex`. `cargo fmt --check`/`clippy --all-targets -D warnings`/`test` clean (39 lib + 7 spike = 46 total). Note: Go `config.go` has no device-credential storage (only the `credentialInactivityTtlSeconds` TTL setting) — device credentials live in the `pairing` package (S-PAIRING story).
- **07-15** — Rust port S-PATHUTIL complete: `src/pathutil/mod.rs` ports `internal/pathutil.SafeJoin` (lexical traversal rejection — clean, reject absolute, reject any `..` component, containment) plus the symlink-resolution half of `internal/workspace.{safeJoin,resolveSymlinks,isWithinRoot}`. Public API: `clean_path(&Path, &str) -> Result<PathBuf, PathError>`, `resolve_symlink(&Path, &Path) -> Result<PathBuf, PathError>`, `PathError` enum (`TraversalAttempted`/`SymlinkEscapesRoot`/`InvalidPath`/`IoError`, thiserror). Symlink-at-final-component and symlinked-parent both rejected outright (matches Go policy — no reads/writes through agent-created links). 20 unit tests via `tempfile` isolation cover `../../etc/passwd`, `../sibling`, `foo/../../../bar`, absolute inputs, `..foo` filename (allowed), empty/NUL edge cases, symlink-outside-root, symlinked-parent escape, symlink chains, non-existent write targets, and `is_within_root` boundary (`/tmp/ws` vs `/tmp/ws-evil`). Dev-dep `tempfile` v3.27.0 added. `cargo fmt --check`/`clippy --all-targets -D warnings`/`test` clean (26 unit + 7 spike = 33 total).
- **07-15** — Rust port S-ACP-SPIKE complete: pinned `agent-client-protocol` v1.2.0 (features `unstable` + `unstable_mcp_over_acp`) + dev-dep `async-process` v2.5.0; `tests/spike_acp.rs` (7 passing) verifies initialize, session/new, prompt streaming (typed `SessionUpdate`), file read/write callback handlers, `terminal/*` shell family (no `ExecuteCommand` in ACP), permission response shape, cancellation + child teardown, MCP-over-ACP relay types (**Go SDK `mcp/message` gap CLOSED** — inline workaround droppable), and auth flow shape. Verified API surface in `docs/rust-ecosystem/acp-rust-sdk.md`. `cargo fmt --check`/`clippy -D warnings`/`test` clean. Real-agent E2E + real PKCE deferred to S-ACP-CORE.
- **07-15** — S-CONTRACT Go compatibility fixture harness complete: `tests/contract/go-fixtures/` captures golden REST/WS/DTO/CLI fixtures from the live Go daemon into `tests/contract/golden/{rest,ws,dto,cli}/`. In-process daemon via `httptest` + `LOCAL_AGENT_STATE_DIR` isolation; redaction policy scrubs secrets/paths/timestamps/long IDs to stable placeholders. `internal/daemon` exposes `Server()`/`Close()` and `internal/config` honors `LOCAL_AGENT_STATE_DIR` for in-process testing. `go test ./tests/contract/go-fixtures/ -run TestGenerateFixtures` regenerates. Rust differential runner deferred to a future story. See `tests/contract/README.md`.
- **07-15** — Rust port S-ARCH complete: single Cargo package at repo root with `src/{app,acp,api,config,events,files,pairing,permissions,search,shell,sync,workspace,interfaces}/` mirroring Go `internal/`; MSRV 1.92.0 pinned (`rust-toolchain.toml` + `Cargo.toml`); deps pinned with 2026-07-15 verification dates (tokio, axum 0.8, tower, tower-http, tower_governor, rustls+aws-lc-rs, tokio-rustls, reqwest, serde, serde_json, toml, rusqlite bundled, tracing, tracing-subscriber, tracing-appender, clap, anyhow, thiserror); `src/app/tls.rs` installs aws-lc-rs provider (tested at startup); `src/app/rate_limit.rs` tower_governor stub; `src/app/logging.rs` tracing-appender to `~/.local-agent/logs/`; `.github/workflows/rust-ci.yml` (fmt/clippy -D warnings/test on Linux, build on macOS, build+test on Windows); `Cargo.lock` committed. `cargo fmt --check`, `cargo clippy -- -D warnings`, `cargo test` (6 passed), `cargo build` all clean. ACP SDK + rust-embed/notify/qrcode/lru/uuid/rand/sha2/rcgen deferred to the stories that first need them.
- **07-13** — Hardened Rust-port plan: added Phase 0 architecture/ACP/contract gates, existing-state migration story, split ACP implementation stories, corrected service dependencies, and parity-first security/release requirements. No Rust implementation started.
- **07-12** — Resolved 2026-07-11 review backlog (22 findings): 12 fixed, 7 wontfix, 2 deferred (now resolved). Key fixes: atomic+durable file writes (config/store/mcp), `PermissionManager` interface completed + `server.Deps` switched to interfaces, `acp.Client` constructor refactored to `ClientConfig` (eliminated 6 `Set*` temporal-coupling calls), custom JSON parser in search replaced with `encoding/json`, magic-byte detection replaced with `http.DetectContentType`, daemon wiring extracted into focused helpers, `useFileChangeDetection` hook extracted from App.tsx, ChatPanel props consolidated into `ChatPanelActions` facade, shared `isSessionNotFound` helper, MCP config functions moved onto `api` object, markdown prose classes deduplicated, dead frontend raw-ID heuristic removed. Deferred then resolved: `agentRegistry` extracted from `acp.Client` (dedicated `RWMutex`, Client facade preserved), `useBackend` actions stabilized with `useCallback` and 3 consumer `eslint-disable` suppressions removed. Verified: `go test -race ./internal/acp`, `go vet ./...`, `go test ./...`, `npm run lint`, `npm run build` all pass. Wontfix: LRU cache, ring buffer, BIP-39 word list, localStorage/theme hooks, duplicate-frontend-types (already resolved), system-messages nil-guard (already refactored).
- **07-12** — Resolved 2026-07-06 review backlog (9 remaining findings, all ACP-related): attachment translation tests added, silent readfile error now logged, session-transport data race fixed (capture transport under lock before goroutine), 4 audit doc fixes (spec ref, user_message_chunk wording, Gap/Deviation labels, permission key 4-tuple), session/list added to spec.md.
- **07-13** — Binary file previews: raw file endpoint (`GET /api/workspaces/{id}/raw`), FileViewer component with PDF/DOCX/STL/video/audio/image viewers. Three.js + mammoth dynamically imported.
- **07-13** — MCP-over-ACP (P4.10) investigated → blocked on SDK, deferred (docs only).
- **07-12** — ACP provider management (P4.11): `providers/list|set|disable`, REST `/api/sessions/{id}/providers`, Settings UI. SPA offline service worker. Dual HTTP/HTTPS listeners. Device-credential expiry clarified. Devin subagent skills + exec-guard security tooling.
- **07-12** — ACP AdditionalDirectories (P4.5) multi-root; MCP health UX (green/red/gray dots); Devin ACP auth + quieter autodetect logs.
- **07-11** — Auth tiers (grace-period revocation + workspace-registration gating); backend security audit (9/11 fixed); remote device auth fix; responsive workspace switcher/tab bar; model switch without history reset; tool-error surfacing + autoscroll fix.
- **07-10** — MCP config backend (WI-1) + settings-as-tab MCP UI (WI-2) + overflow toggle (WI-4); ACP autodetect/init diagnostics.
- **07-08** — Chat UI restructure (WI-3: ChatTabBar/Composer/ConversationView); chat-tab regressions; ACP spec compliance P1+P2 (structured resource blocks, stop reason, open-files context, reject_always, terminal env/signal, pinned protocol version).
- **07-06** — Image upload flow; light theme + shadcn foundation; frontend hooks hardening; live file-change detection (`internal/fswatch`); all 25 review findings resolved.

## Verification

| Check | Status |
|-------|--------|
| `go build ./...` / `go vet ./...` | ✅ Pass |
| `go test ./...` | ✅ Pass |
| `npm run lint` / `npm run build` (web/) | ✅ Pass |
| `./build.sh` / `.\build.ps1` (embed + compile) | ✅ Pass |
| Runtime: `app start` → `/health` 200 → clean stop | ✅ 2026-07-06 |
| Runtime: ACP E2E; upload round-trip | ✅ 2026-07-06 |

## Rust Port

| Check | Status |
|-------|--------|
| `cargo fmt --check` / `cargo clippy -- -D warnings` / `cargo test` / `cargo build` | ✅ 2026-07-15 (S-ARCH) |
| Multi-OS CI (Linux fmt/clippy/test, macOS build, Windows build+test) | ✅ Workflow added; first run on push |
| rustls aws-lc-rs provider installs at startup | ✅ Unit test |
| File logging → `~/.local-agent/logs/` | ✅ Stub wired |
| ACP Rust SDK pinned | ✅ 2026-07-15 (S-ACP-SPIKE) `agent-client-protocol` v1.2.0 |
| S-CONTRACT Go golden fixtures (REST/WS/DTO/CLI) | ✅ 2026-07-15 — `go test ./tests/contract/go-fixtures/ -run TestGenerateFixtures` |
| S-CONTRACT Rust differential runner | ⛔ Future story (Rust daemon not yet implemented) |
| Service module implementations | ⛔ Phase 1+ stories |

## Key Decisions

- Go module `github.com/adama/local-agent`; frontend dir `web/`.
- Multi-device, single-user (no multi-tenant surface).
- Dark theme default; Tailwind v4 `@theme inline`; event-driven UI (render derives from `AppEvent[]`).
- SQLite via `modernc.org/sqlite` (pure-Go, no CGO).
- ACP is the only agent integration path — no per-agent code; client owns fs/shell/permissions/state.
- Must rebuild binary after frontend changes (`go:embed` freezes `internal/server/dist` at compile time).
- `ACPClient.SendPrompt` taking `[]Attachment` is a breaking change for out-of-tree implementers.
