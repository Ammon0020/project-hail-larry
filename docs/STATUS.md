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
- [ ] **Rust backend port** — Phase 0 done (S-ARCH/S-CONTRACT/S-ACP-SPIKE). Phase 1: S-PATHUTIL/S-CONFIG/S-INTERFACES/S-EVENTS done. Shared **`src/fsutil`**. Next: S-MIGRATE.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`). Inline transport retained. Rust SDK closes this gap. Docs: `acp-spec-compliance.md` §4.10, `known-issues.md`.

## Recent Changes (2026-07)

- **07-15** — Rust port **S-EVENTS** complete: `src/events/{mod,store,publisher,payload,tests}.rs`. SQLite WAL store (`rusqlite` bundled), single-connection + `spawn_blocking` boundary, schema matching Go (`events` table + indexes), `Append`/`Query`/`QueryAll`, prune by count/age + ticker. `EventBus`: persist-before-publish, subscribe→replay→dedupe-by-ID→live. PRAGMAs: `journal_mode=WAL`, `busy_timeout=5000`. Tests: 27 events (+ payload unit) + prior suite = 99 lib + 7 spike = 106. Next: S-MIGRATE.
- **07-15** — Rust port **S-INTERFACES** complete: DTOs/traits/wire/`AppError`. Deps: `async-trait`, `chrono`.
- **07-15** — Daemon start resilience; shared fsutil; S-CONFIG; S-PATHUTIL; S-ACP-SPIKE; S-CONTRACT; S-ARCH.
- **07-13–07-06** — Chat/MCP/auth/ACP provider + binary previews; review backlog resolved; MCP-over-ACP Go SDK gap documented.

## Known Gaps (summary — see `docs/known-issues.md`)

- Go ACP SDK still missing `mcp/message` relay (Rust path unblocked).
- Mobile editor touch; profile mode not wired; pair QR scheme selection.
- Rust port Phase 1 remaining: S-MIGRATE; Phase 2+ service ports.
