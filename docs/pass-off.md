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

**Done:**
- `docs/plan.md` created with 13 Phase 1 tasks
- Frontend ported from `mockup13.html` → React + TypeScript in `web/`
- Tailwind v4 + shadcn/ui setup, `@theme` tokens, `@` path alias
- Components: LockScreen, ActivityBar, LeftSidebar, FileTree, SearchPanel, EditorPane (CodeMirror 6), ChatPanel, ChatHistory, ChatMessageItem, MobileNav, MobileSettings
- Mock data layer (`web/src/data/mockData.ts`) and mock backend hook (`web/src/hooks/useMockBackend.ts`)
- `npm run build` passes ✅
- Directory structure created: `cmd/`, `internal/{daemon,config,workspace,pairing,acp,events,permissions,shell,sync,files,interfaces}/`, `web/`

**Blocked:**
- Go is not installed on this machine. Cannot proceed with `go mod init`, HTTP server, `go:embed`, or shared interfaces.
- Install Go: `winget install GoLang.Go` or download from https://go.dev/dl/
- After install, run `go mod init github.com/adama/local-agent`

## Next Steps (in order)

1. Install Go, run `go mod init github.com/adama/local-agent`
2. Add deps from TechStack.md: cobra, gorilla/websocket, mattn/go-sqlite3, etc.
3. Write `cmd/app/main.go` — HTTP server on `0.0.0.0:7337` with `/health` endpoint
4. Wire `go:embed` to serve `web/dist/` in production
5. Define shared interfaces in `internal/interfaces/` (EventStore, WorkspaceManager, ACPCClient, PermissionManager)
6. Add Vite proxy config in `web/vite.config.ts` → proxy `/api` and `/ws` to `localhost:7337`
7. Verify: `go build ./...`, `go test ./...`, `go vet ./...`, `npm run build`
8. Mark `scaffold` as `[x]` in `docs/plan.md`
9. Spawn subagents per spawn order in `first_agent.md` Sec 4

## Key Decisions

- Frontend dir is `web/` (not `frontend/`) per `first_agent.md` structure
- Go module path: `github.com/adama/local-agent`
- Dark theme is default; `document.documentElement.classList.add('dark')` in `main.tsx`
- Theme tokens use Tailwind v4 `@theme inline` pattern (CSS vars in `:root`/`.dark`, mapped in `@theme inline`)
- Custom colors: `background`, `panel`, `activity-bar`, `editor`, `status-bar`, `tool-call` (beyond standard shadcn tokens)
- Event-driven UI: all chat/tool/permission rendering derives from `AppEvent[]` stream (Blueprint Sec 11)
- `useMockBackend` hook simulates WebSocket + ACP; replace with real `ws://` connection when `ws-sync` task is done

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
