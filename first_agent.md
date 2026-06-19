# First Agent — Orchestrator Instructions

ONLY for the first orchestrator agent. Your job is to bootstrap the Local Agent Interface project, then spawn subagents to build it in parallel. You write the groundwork, delegate the rest, and verify everything compiles and tests pass before declaring any phase complete.

---

## 1. Read the Documentation

Read these files in order before doing anything:

1. `docs/Blueprint.md` — full architecture and design. This is the source of truth.
2. `docs/TechStack.md` — technology decisions and library choices.
3. `docs/OpenItems.md` — known gaps and deferred decisions. Do not implement deferred items. If you encounter a blocker that maps to an open item, note it and move on.
4. `mockup12.html` — the UI mockup. The frontend agent should reference this closely.

Do not proceed to planning until you have read all four.

---

## 2. Create the Task Plan

Create a plan file at `docs/plan.md` that splits Phase 1 (Core Infrastructure) from the Blueprint into executable subagent tasks. Each task must be self-contained enough that a subagent can execute it without reading another task's instructions.

### Plan Structure

Each task entry should contain:

- **Name** — short, descriptive task name
- **Scope** — what files/packages this agent owns. Be explicit about boundaries so agents don't collide.
- **Dependencies** — which tasks must complete first (if any)
- **Blueprint references** — section numbers from `docs/Blueprint.md` that define this work
- **Acceptance criteria** — what must be true for this task to be considered done (including tests passing)

### Task Split

The following is the recommended split. Adjust if you see obvious coupling or conflict, but keep each task in one lane:

| Name | Owns | Deps | Blueprint Refs |
|---|---|---|---|
| scaffold | Go module, directory structure, `go:embed` setup, Vite frontend scaffold, basic HTTP server | None | Sec 3, 25 (Phase 1) |
| cli-daemon | `cmd/` CLI commands (cobra), daemon start/stop/status, config storage in `~/.local-agent/` | scaffold | Sec 4 (Host Daemon), Sec 20 (Configuration) |
| pairing | Pairing sessions, QR generation, mnemonic passcode, device credentials, lock screen API | cli-daemon | Sec 19 (Authentication) |
| workspace | Workspace registration, file tree, git info, workspace config | cli-daemon | Sec 13 (Workspace Management), Sec 14 (File System Access) |
| events | Event types, SQLite schema, event append/query, state derivation | scaffold | Sec 11 (Event System) |
| acp-client | ACP transport (stdio JSON-RPC), session lifecycle, prompt exchange, streaming, capability negotiation | workspace, events | Sec 6 (ACP Client Layer), Sec 7 (ACP Integration), Sec 9 (Agent Lifecycle), Sec 10 (Session Lifecycle) |
| permissions | `session/request_permission` handling, prompt routing to all devices, allow/deny policies, audit log | events, acp-client | Sec 8 (Permission Manager) |
| shell-exec | Workspace-scoped subprocess runner, output streaming as events | acp-client, permissions | Sec 15 (Shell Execution) |
| ws-sync | WebSocket server, event broadcast, reconnection with missing-event sync, in-flight permission prompt re-presentation | events | Sec 12 (Multi-Client Synchronization) |
| file-sync | Revision tracking, `FileRevisionUpdated` events, three-way merge on save, live agent change indicator | workspace, events | Sec 14 (File System Access — Client File Sync) |
| frontend-shell | React app shell, layout (sidebar, main area, mobile nav), chat/event stream view, command input, session list | scaffold, ws-sync | Sec 17 (UI Architecture), mockup12.html |
| frontend-editor | CodeMirror 6 editor pane, file tree, diff view, merge UI, file save with `expectedRevision` | file-sync, frontend-shell | Sec 14, Sec 17 (Editor and File Viewing), mockup12.html |
| frontend-pairing | Lock screen, pairing flow (QR scan / mnemonic entry), permission dialog UI, settings panel | pairing, permissions, frontend-shell | Sec 8, Sec 19, mockup12.html |

### Rules for the Plan

- **Keep related info close.** Tasks that share data models or interfaces should reference each other's interface definitions, not reimplement them.
- **Stay in your lane.** Each task owns specific packages/files. If an agent needs something from another lane, it defines an interface and depends on it — it does not implement the other agent's code.
- **Update the plan as you go.** If a task turns out to be larger than expected, split it. If two tasks are tightly coupled, merge them. Note changes in `docs/plan.md`.
- **Track progress.** Mark tasks as `[ ]` → `[~]` (in progress) → `[x]` (done) in `docs/plan.md` as work proceeds.

---

## 3. Write the Groundwork (scaffold)

Do this yourself before spawning any subagents:

1. **Go module** — `go mod init github.com/adama/local-agent`. Add dependencies from `docs/TechStack.md`.
2. **Directory structure:**
   ```
   cmd/           # CLI entry point (cobra)
   internal/
     daemon/      # daemon lifecycle
     config/      # configuration management
     workspace/   # workspace management
     pairing/     # device pairing & auth
     acp/         # ACP client layer
     events/      # event system & SQLite persistence
     permissions/ # permission manager
     shell/       # shell execution
     sync/        # WebSocket sync
     files/       # file sync & merge
   web/           # React frontend (Vite)
   ```
3. **Frontend scaffold** — `npm create vite@latest web -- --template react-ts`, add TailwindCSS, configure Vite proxy to Go backend on `:7337`.
4. **Basic HTTP server** — Go server on `0.0.0.0:7337` serving a health check. Wire up `go:embed` for production builds.
5. **Shared interfaces** — define Go interfaces for packages that will be implemented by subagents (event store, workspace manager, ACP client, permission manager). These are the contracts between lanes.
6. **Test that it compiles and runs.** `go build ./...` and `go test ./...` must pass. The frontend should `npm run build` successfully.

---

## 4. Spawn Subagents

Spawn subagents one at a time or in dependency order. For each subagent:

1. Give it the task entry from `docs/plan.md` (its scope, dependencies, blueprint refs, acceptance criteria).
2. Tell it to read the referenced Blueprint sections and any interface definitions from scaffold.
3. Tell it to write tests for its code and run `go test ./...` and `go vet ./...` before declaring done.
4. Tell it to update `docs/plan.md` with progress (`[~]` when starting, `[x]` when complete).
5. After each subagent completes, verify:
   - `go build ./...` passes
   - `go test ./...` passes
   - `go vet ./...` passes
   - `npm run build` passes (for frontend tasks)
   - No files outside the agent's declared scope were modified

### Spawn Order

```
scaffold (you) → cli-daemon, events (parallel)
              → pairing, workspace (parallel, after cli-daemon)
              → acp-client (after workspace, events)
              → permissions, ws-sync, file-sync (parallel, after acp-client / events)
              → shell-exec (after permissions)
              → frontend-shell (after ws-sync)
              → frontend-editor (after file-sync, frontend-shell)
              → frontend-pairing (after pairing, permissions, frontend-shell)
```

---

## 5. Quality Bar

- **Test before complete.** Every package must have tests. Run `go test ./...` before marking any task done.
- **Lint regularly.** Run `go vet ./...` after each task. Run `npm run build` after frontend tasks.
- **Fail loudly.** If a subagent's code doesn't compile or tests fail, fix it or re-spawn the agent with specific feedback. Do not mark it complete.
- **Plans are living documents.** If something in the Blueprint is obviously wrong or missing during implementation, update `docs/plan.md` with a note and proceed with the best approach. Do not silently deviate.
- **Scope: Phase 1 only.** Do not implement Phase 2 or Phase 3 features. If a subagent tries to, redirect it.

---

## 6. Completion

Phase 1 is complete when:

- All tasks are marked `[x]` in `docs/plan.md`
- `go build ./...`, `go test ./...`, `go vet ./...` all pass
- `npm run build` passes
- The daemon starts with `app start`, serves the web UI, and a browser can connect
- A workspace can be registered, a device can be paired, and the lock screen appears for unpaired devices
- The chat view, editor pane, and file tree render in the UI
