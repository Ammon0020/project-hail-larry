# Pass-Off — Next Session

## Project

Local Agent Interface — self-hosted web code editor with AI. Go daemon serves a React IDE to devices on LAN. Uses ACP to orchestrate external agents (Claude Code, Codex, Gemini, etc.).

## Read These First

1. `AGENTS.md` — project rules and vision
2. `first_agent.md` — orchestrator instructions (your playbook)
3. `docs/plan.md` — Phase 1 task breakdown with status
4. `docs/plans/Blueprint.md` — full architecture (source of truth)
5. `docs/development/TechStack.md` — library choices

## Current State

**Phase 1 COMPLETE.** All 14 tasks done (13 planned + integration). See `docs/STATUS.md` for per-task status.

**Verification (2026-06-20):**
- `go build ./...` ✅
- `go test ./...` ✅ (all packages)
- `go vet ./...` ✅
- `npm run build` ✅

**What's been built:**
- Go 1.26.4 installed, module `github.com/adama/local-agent`
- All internal packages: daemon, config, server, events, pairing, workspace, acp, permissions, sync, files, shell, interfaces
- HTTP server on `0.0.0.0:7337` with full REST API + WebSocket `/ws`
- Frontend embedded via `go:embed` in `internal/server/dist/`
- Frontend wired to real backend via `useBackend` hook (replaced mock)
- Cobra CLI: start, stop, status, add-folder, pair, devices, revoke, logs

## Next Steps

1. **Runtime verification** — `app start`, pair a device, verify UI loads and chat/editor/file-tree work
2. **Open items** — TLS on LAN, pairing TTL config (see `docs/plans/OpenItems.md`)
3. **Phase 2** — per `docs/plans/Blueprint.md` (don't start until runtime verified)

## Key Decisions

- Frontend dir is `web/` (not `frontend/`) per `first_agent.md` structure
- Go module path: `github.com/adama/local-agent`
- Dark theme is default; `document.documentElement.classList.add('dark')` in `main.tsx`
- Theme tokens use Tailwind v4 `@theme inline` pattern (CSS vars in `:root`/`.dark`, mapped in `@theme inline`)
- Custom colors: `background`, `panel`, `activity-bar`, `editor`, `status-bar`, `tool-call` (beyond standard shadcn tokens)
- Event-driven UI: all chat/tool/permission rendering derives from `AppEvent[]` stream (Blueprint Sec 11)
- `useBackend` hook connects to real Go backend via REST + WebSocket (replaced former `useMockBackend`)

## File Map

```
web/
  src/
    App.tsx              # App shell — orchestrates all panels
    main.tsx             # Entry, applies dark theme
    index.css            # Tailwind v4 @theme tokens
    lib/utils.ts         # cn() class merge
    types/index.ts       # TypeScript types
    data/mockData.ts     # Mock data (file tree, agents, sessions, events)
    hooks/useMockBackend.ts  # Simulates daemon WebSocket
    components/           # All UI components (11 files)
  vite.config.ts         # React + Tailwind plugin + @ alias (needs proxy config added)
  tsconfig.app.json      # Path aliases configured
  package.json           # React 19, Vite 8, Tailwind 4, CodeMirror, lucide-react
```
