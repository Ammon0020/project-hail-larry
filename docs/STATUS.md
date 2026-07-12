# Project Status — Local Agent Interface

> Last updated: 2026-07-13. Source of truth for task-level status. Keep < 150 lines.
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
- **Security** — auth middleware on all routes, WS Origin/CSRF checks, path-traversal + symlink containment, rate limiting, request/size caps. Full audit 2026-07-07 (9/11 fixed; 2 deferred in known-issues).

## Active TODO

- [ ] **Editor on mobile** — CodeMirror touch optimization.
- [ ] **Profile mode (Code/Ask/Plan)** — composer selector is a UI placeholder; needs to be sent with the prompt.
- [ ] **MCP store/settings icons** — popout header icons are no-ops (settings icon → MCP settings; store icon → future marketplace).
- [ ] **QR/pair scheme selection** — `app pair`/QR encode the HTTPS URL only; let device pick, or encode both.
- [ ] **Multi-user vs multi-device** — decided multi-device/single-user; multi-user/team collaboration remains future.
- [ ] **ACP futures** — sub-workers, session fork/resume/close, elicitation, NES, audio, ACP-inspector (see `acp-spec-compliance.md`).
- [ ] **Phase 2 (Multi-Agent)** — multiple simultaneous workers, capability negotiation, enhanced diagnostics.

## Blocked

- **MCP-over-ACP (P4.10)** — ⛔ SDK gap: `acp-go-sdk` v0.13.5 doesn't code-generate the `mcp/message` relay (only `mcp/connect`/`mcp/disconnect`) and it can't be wired via the stock `ClientSideConnection`. Inline transport retained. Blocker + drop-in design: `acp-spec-compliance.md` §4.10 and `known-issues.md`.

## Recent Changes (2026-07)

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

## Key Decisions

- Go module `github.com/adama/local-agent`; frontend dir `web/`.
- Multi-device, single-user (no multi-tenant surface).
- Dark theme default; Tailwind v4 `@theme inline`; event-driven UI (render derives from `AppEvent[]`).
- SQLite via `modernc.org/sqlite` (pure-Go, no CGO).
- ACP is the only agent integration path — no per-agent code; client owns fs/shell/permissions/state.
- Must rebuild binary after frontend changes (`go:embed` freezes `internal/server/dist` at compile time).
- `ACPClient.SendPrompt` taking `[]Attachment` is a breaking change for out-of-tree implementers.
