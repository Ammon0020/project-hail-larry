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
  acp/                   → ACP (Agent Client Protocol) client using coder/acp-go-sdk
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
docs/                    → see ## Docs section below
```

## Architecture Rules

- ACP is the communication protocol with agents — no per-agent integration code
- Client owns filesystem, shell execution, permissions, and session state
- UI never communicates directly with agent implementations
- Agents plan and propose; the client executes approved actions
- Stay in your lane — define interfaces, don't implement another agent's code

## Subagents

Task delegation to specialized subagents is a core workflow and saves time. For non-trivial features, decompose the work and delegate rather than doing everything serially in one context. Each subagent's description (auto-inserted into the prompt) states which difficulty tier and task type it's suited for — match subtasks to the right tier and dispatch them, in parallel where possible, rather than working through everything yourself.

Note that if commands are rejected in succession on a subagent, it's likely because only certain commands are auto-approved. Subagents should skip them and tell the parent. 

If the quota runs out, use the small subagent. It's still very powerful and can handle most tasks. Trivial will continue to function as well. 

## Development Standards

- **Build:** Build with `.\build.ps1` (Windows) or `./build.sh` (Linux/macOS). Both build the frontend, re-embed `internal/server/dist`, then compile the Go binary. `internal/server/dist/` is gitignored, so a frontend build must run before `go build` can embed assets (a fresh checkout also needs `npm install` in `web/`).
- **Test:** Run tests and linting before marking a task complete — `go test ./...`, `go vet ./...`, `npm run build`. Use quiet flags where possible to only get essential output.
- **Stay on task:** If a test fails in another task, note it in `docs/known-issues.md` and move on
- **Plans:** Must be a summary (see `docs/plans/Blueprint.md`) or executable in one branch under a single work item (see `docs/plans/`)
- **No inline CSS or hard-to-read styling:** Use Tailwind classes and keep styles in components. Use `cva` for elements with 2+ visual variants; leave one-off styles inline.
- **Use linters:**
  - `golangci-lint` for Go
  - `eslint` for JavaScript/TypeScript

- **Keep `docs/STATUS.md` current.** When you start, modify, or complete a task, update the relevant row in STATUS.md immediately. Mark gaps honestly — "⚠️ Partial" or "⚠️ Stub" over false "✅ Done". Include short notes on what's missing. Compact occasionally. 

## Security

Security is of utmost importance. This daemon serves a browser UI to devices on the local network, executes shell commands, and writes files on behalf of AI agents — a capable attack surface.

- **Periodic security audits:** Run a focused security audit (subagent) at least once per major feature batch or before any release. Audit auth, path traversal, command injection, input validation, secrets, TLS, SQL injection, and DoS. Store findings under `docs/reviews/<date>/`.
- **Track deferred findings:** Findings not fixed immediately go in `docs/known-issues.md` with severity and a concrete fix path — never silently dropped.
- **No secrets in logs, configs, or commits.** Device credentials are hashed at rest; never log raw tokens, passcodes, or secrets.
- **Default to secure:** TLS on by default, bind to 0.0.0.0 only with TLS, reject symlinks in workspace paths, rate-limit unauthenticated endpoints, cap request/response sizes.
- **When adding new endpoints or file/command surfaces:** Consider the threat model — who can call this (loopback vs. any LAN device vs. any paired device), and what's the worst case if abused.

## Agent Hints
- LLM Memory is limited by context length. Long chats can cause context overflow. Keep terminal output concise and focused where possible by using quiet flags, filtering output, and capping linter output to a few errors at a time.
- When delegating to subagents, provide clear task boundaries and expected outputs

## Tailwind CSS Standards

- Utility classes live in JSX, not separate CSS files — co-located for traceability
- Use semantic design tokens (`text-muted-foreground`, `bg-background`), not raw color values
- Extract repeated class patterns into components or `cva` variants, not `@apply` classes
- `@apply` only for third-party HTML you don't control
- One global CSS file (`src/index.css`) for theme tokens and `@theme` definitions only
- Mobile-first: unprefixed = base, `md:`/`lg:` add up
- Dark mode via `dark:` prefix or `data-theme` attribute, never JS conditionals
- Order classes consistently: layout → spacing → typography → color → state
- Use `clsx` or `tailwind-merge` (`cn` utility) for dynamic/conditional styling to avoid conflicts

## Docs

- **[AGENTS.md](AGENTS.md)** — Core agent rules and coordinating information (this file)
- **[docs/plans/Blueprint.md](docs/plans/Blueprint.md)** — Core agent design and mutual understanding of the app (the design source of truth)
- **[docs/STATUS.md](docs/STATUS.md)** — Central task-level implementation status, checklists, and active codebase gaps. Never over 150 lines. 
- `docs/known-issues.md` — Deferred review findings and tracked gaps
- `docs/development/TechStack.md` — Technology choices and library list
- `docs/reference/<topic>/` — Stable reference material for external standards and tools we conform to (e.g. `reference/acp/` for the Agent Client Protocol spec)
- `docs/plans/` — Executable technical blueprints and work plans for complex tasks (e.g. `OpenItems.md`, `execution-plan.md`, `acp-spec-compliance.md`)
- `docs/reviews/<date>/` — Dated audit snapshots and review findings
- `docs/specs/` — Internal feature specs (backend, chat panel, UI)
- `mockup12.html` — UI mockup for frontend agents