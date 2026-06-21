# AGENTS.md

Rules and critical info posted at the start of every chat to create context.

## Vision

Self-hosted web code editor with AI built in. A Go daemon runs on the user's machine, serving a browser-based IDE to any device on the local network. The app has an ACP (Agent Client Protocol) client that orchestrates external agents (Claude Code, Codex CLI, Gemini CLI, etc.) alongside a VS Code-style editor. All files and state stay on the host; devices are thin clients synced in real time. Cross platform (Windows, Mac, and Linux).

## Workflow

- **Setup:** `app add-folder .` registers a workspace, `app pair` generates a QR code
- **First device:** Scan QR → paired → web UI opens
- **Additional devices:** Navigate to local IP → lock screen → `app pair` → enter four-word mnemonic passcode → paired
- **Coding:** Pick harness + model in right panel, type instructions next to code. Agent reads files, proposes edits, runs shell commands, streams responses into chat
- **File sync:** Agent edits show live diff indicators; user edits trigger revision tracking + three-way merge to prevent conflicts
- **Permissions:** Inline prompts for shell commands / file writes — approve from any paired device
- **Mobile:** Bottom-nav layout — one panel at a time (explorer, editor, chat, settings)

## Project Layout

```
cmd/app/                 → CLI entry point (cobra commands)
internal/
  daemon/                → Daemon lifecycle, wires all managers
  server/                → HTTP server, go:embed frontend, REST API, /ws
  config/                → Config storage in ~/.local-agent/
  events/                → SQLite event store (WAL, append-only)
  pairing/               → QR + mnemonic pairing, device credentials
  workspace/             → Registration, file tree, git info
  acp/                   → ACP client (stdio JSON-RPC to agents)
  permissions/           → Permission request/response, policies
  sync/                  → WebSocket hub, broadcast, reconnection
  files/                 → Revision tracking, three-way merge
  shell/                 → Workspace-scoped subprocess runner
  interfaces/            → Shared Go interfaces (EventStore, ACPClient, etc.)
web/                     → React 19 + Vite 8 + Tailwind v4 + shadcn/ui
  src/components/        → UI components
  src/hooks/             → useBackend (real backend), useMockBackend
  src/lib/               → api.ts (REST client), utils.ts
  src/data/              → Mock data
  src/types/             → TypeScript types
docs/                    → Blueprint, plan, status, open items
```

## Architecture Rules

- ACP is the communication protocol with agents — no per-agent integration code
- Client owns filesystem, shell execution, permissions, and session state
- UI never communicates directly with agent implementations
- Agents plan and propose; the client executes approved actions
- Stay in your lane — define interfaces, don't implement another agent's code

## Development Standards

- Run tests and linting before marking a task complete — `go test ./...`, `go vet ./...`, `npm run build`
- Stay on task — if a test fails in another task, note it in `docs/known-issues.md` and move on
- Plans must be summary (see `docs/Blueprint.md`) or executable in one branch under a single work item (see `docs/plans/`)
- No inline CSS or hard-to-read styling
- Use linters: 
 - `golangci-lint` for Go
 - `eslint` for JavaScript/TypeScript
- **Keep `docs/STATUS.md` current.** When you start, modify, or complete a task, update the relevant row in STATUS.md immediately. Mark gaps honestly — "⚠️ Partial" or "⚠️ Stub" over false "✅ Done". Include short notes on what's missing.

Note: Cap lint output — fix a few errors per pass, not the whole dump.

## Tailwind CSS Standards

- Utility classes live in JSX, not separate CSS files — co-located for traceability
- Use semantic design tokens (`text-muted-foreground`, `bg-background`), not raw color values
- Extract repeated class patterns into components or `cva` variants, not `@apply` classes
- `@apply` only for third-party HTML you don't control
- One global CSS file (`src/index.css`) for theme tokens and `@theme` definitions only
- Mobile-first: unprefixed = base, `md:`/`lg:` add up
- Dark mode via `dark:` prefix or `data-theme` attribute, never JS conditionals
- Order classes consistently: layout → spacing → typography → color → state

## Docs

- `docs/Blueprint.md` — full architecture and design (source of truth)
- `docs/development/TechStack.md` — technology choices and library list
- `docs/plans/Blueprint.md` — phased implementation plan
- `docs/plans/OpenItems.md` — tracked gaps and deferred decisions; do not implement deferred items
- `docs/STATUS.md` — task-level status tracking; keep updated as work progresses
- `mockup12.html` — UI mockup; frontend agents should reference this closely