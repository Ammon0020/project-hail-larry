# Prompt: Finish the Local Agent Interface Codebase

> Copy everything below the line into a new LLM session (Claude Code, Codex, etc.).
> This LLM will act as **team lead** and dispatch subagents for each work stream.

---

You are the **team lead** for finishing the Local Agent Interface ("project-hail-larry") codebase. You orchestrate the work by dispatching **subagents** — one per work stream — and reviewing their output. You do not write feature code yourself; you coordinate, review, and integrate.

## Project Summary

A self-hosted web code editor with AI built in. A Go daemon runs on the user's machine, serving a browser-based IDE to any device on the local network. The app uses the **Agent Client Protocol (ACP)** to orchestrate external agents (Claude Code, Codex CLI, Gemini CLI, etc.) alongside a VS Code-style editor. All files and state stay on the host; devices are thin clients synced in real time. Cross-platform (Windows, Mac, Linux).

**Stack:** Go 1.21+ (`github.com/adama/local-agent`), React 19 + Vite 8 + Tailwind v4 + shadcn/ui, SQLite (pure-Go `modernc.org/sqlite`), ACP via `github.com/coder/acp-go-sdk@v0.13.5`.

## Critical Rules (from AGENTS.md)

- **Build:** Run `.\build.ps1` after any frontend change — `go:embed` freezes the frontend into the binary at compile time.
- **Test gates:** `go test ./...`, `go vet ./...`, `npm run build`, `npm run lint` — all must pass before marking done.
- **Keep `docs/STATUS.md` current** — update the relevant row when you start, modify, or complete a task. Mark gaps honestly: "⚠️ Partial" over false "✅ Done".
- **ACP is the only agent communication protocol** — no per-agent integration code. Use Context7 to fetch up-to-date ACP docs (resolve `acp go sdk coder`, then call `get-library-docs`).
- **Stay in your lane** — define interfaces, don't implement another agent's code.
- **No inline CSS.** Use semantic Tailwind tokens. Classes live in JSX.
- **Cap lint output** — fix a few errors per pass, not the whole dump.

## Most Accurate Docs (read these first)

These documents are the **most accurate** source of truth. Where older docs conflict, these win:

1. **`docs/acp/responsibilities.md`** — verified ACP division of responsibilities (agent vs. client), file write use case, Context7 lookup instructions. Every claim here has been verified against the official ACP spec and the actual codebase.
2. **`docs/acp/overview.md`** — verified ACP overview with correct official URLs.
3. **`AGENTS.md`** — project rules, layout, architecture principles, dev standards.
4. **`docs/STATUS.md`** — current task-level status with honest gap markers (⚠️ Partial).

Older planning docs (`docs/plans/acp-stability.md`, `docs/plans/agent-context.md`, `docs/specs/*`) contain useful detail but may have stale claims. Cross-reference against the docs above.

## Current State

Phase 1 (core infrastructure) is ~90% done. 5 of 14 tasks are overstated. The app builds, tests pass, and the UI works end-to-end with real agents. The remaining work falls into **5 work streams**.

## Step 1: Consolidate Plan Documents (do this yourself, no subagent)

Before dispatching subagents, create a single consolidated execution plan:

1. Read `docs/STATUS.md` — note every ⚠️ Partial row and its gap description.
2. Read `docs/acp/responsibilities.md` — note the 3 ⚠️ _Planned_ items.
3. Read `docs/plans/OpenItems.md` — note which items are in scope (below).
4. Read `docs/plans/agent-context.md` — this is a draft plan for the agent context provider. It's detailed but needs to be reconciled against the actual codebase before implementation.
5. Write `docs/plans/execution-plan.md` — a single document that lists every remaining work item, grouped by work stream, with: scope, files, acceptance criteria, dependencies, and verification command. This is the document your subagents will work from.

## Step 2: Dispatch Subagents

Dispatch one subagent per work stream. Each subagent gets the execution plan section for its stream plus the relevant files. Review their output before moving to the next dependent stream.

### Work Stream 1: ACP Session Lifecycle Completion

**Gaps:**
- `session/load` (ACP `LoadSession`) — not called. Sessions are recreated fresh on restart instead of resumed.
- `session/delete` (ACP `DeleteSession`) — not called. `CloseSession` kills the process but doesn't call ACP delete.
- Graceful `session/close` on daemon shutdown (currently uses kill).

**Scope:**
- `internal/acp/transport.go` — add `LoadSession` and `DeleteSession` wrapper methods on `Transport`.
- `internal/acp/acp.go` — `startTransportLocked`: if session has a persisted `acpSessionID`, try `LoadSession` first; fall back to `NewSession` if the agent doesn't support it or the session is gone. `CloseSession`: call ACP `DeleteSession` before killing the process. Daemon shutdown: iterate sessions, call `CloseSession` gracefully.
- Check `AgentCapabilities.LoadSession` from the initialize response before attempting load.
- Update `docs/STATUS.md` task 6 row when done.
- Update `docs/acp/responsibilities.md` — remove the ⚠️ _Planned_ marker for session/load and session/delete.

**Verify:** `go test ./internal/acp/...`, `go vet ./...`

### Work Stream 2: Permission Policy Enforcement

**Gap:** `allow_always` / `reject_always` decisions are recorded in the audit log but never auto-resolve future requests. Every permission request blocks for user input.

**Scope:**
- `internal/permissions/permissions.go` — add a policy map: `(sessionID, toolKind, target) → decision`. When `Request` is called, check the policy map first. If a matching `allow_always` or `reject_always` exists, return immediately without blocking. Otherwise, block as today and record the decision in the policy map when resolved.
- `internal/permissions/permissions_test.go` — add tests: (1) `allow_always` auto-resolves subsequent same-kind request, (2) `reject_always` auto-denies, (3) `allow_once` does NOT auto-resolve, (4) policy is session-scoped.
- Update `docs/STATUS.md` task 7 row when done.
- Update `docs/acp/responsibilities.md` — remove the ⚠️ _Planned_ marker for permission policy.

**Verify:** `go test ./internal/permissions/...`, `go vet ./...`

### Work Stream 3: Agent Context Provider

**Gap:** Agents receive no workspace context on first prompt — they don't know what files exist, the workspace root, or git status. This causes excessive shell command round-trips just for file discovery.

**Plan:** `docs/plans/agent-context.md` is a detailed draft. The subagent should read it, reconcile it against the actual codebase (the `Transport.Prompt` method in `internal/acp/transport.go`, the `SendPrompt` flow in `internal/acp/acp.go`, and the `WorkspaceManager` interface in `internal/interfaces/interfaces.go`), and implement it.

**Scope:**
- New `internal/acp/context.go` — `FirstPromptContextMiddleware` that injects workspace file tree (capped at N files), git status, and `AGENTS.md` content into the first prompt of a session.
- `internal/acp/acp.go` — wire the middleware into `SendPrompt`: before calling `transport.Prompt`, run the middleware pipeline. If it returns an inject message, prepend it to the prompt content.
- Keep it simple: a middleware interface (`BeforePrompt(ctx, session, prompt) → (action, injectedContent)`), one implementation, no over-engineering.
- Unit tests per the testing section in `docs/plans/agent-context.md`.
- Update `docs/STATUS.md` — mark the agent context draft as implemented.

**Verify:** `go test ./internal/acp/...`, `go vet ./...`

### Work Stream 4: Open Items & Hardening

**Gaps from `docs/plans/OpenItems.md` (in scope for this pass):**
- **TLS on LAN** — plain HTTP exposes pairing tokens and file contents. Add optional TLS support: generate self-signed cert on first run, serve HTTPS, trust-on-first-use for paired devices.
- **Pairing TTL** — currently 5 min hardcoded. Make configurable via `~/.local-agent/config.json`.
- **UI persistence** — on reload, the UI loses state (selected files, active model, active conversation). Persist to `localStorage` and restore on load.
- **Workspace CLI gaps** — no `app remove-folder`, no `app list-folders` commands.

**Scope:**
- `internal/server/server.go` — add TLS listener option, cert generation/storage in `~/.local-agent/`.
- `internal/pairing/pairing.go` — make TTL configurable from config.
- `internal/config/` — add `pairingTTL` and `tlsEnabled` fields to config struct.
- `cmd/app/` — add `remove-folder` and `list-folders` cobra commands.
- `web/src/hooks/useBackend.ts` + `web/src/App.tsx` — persist active workspace, active session, active model, open tabs to `localStorage`; restore on mount.
- Update `docs/STATUS.md` and `docs/plans/OpenItems.md` — check off resolved items.

**Verify:** `go test ./...`, `go vet ./...`, `npm run build`, `npm run lint`, `.\build.ps1`

### Work Stream 5: Chat Panel Mockup & Feature Spec

**Gap:** No comprehensive spec or mockup exists for the right-side chat panel that accounts for every ACP feature — chat streaming, model switching, harness switching, chat locking to a harness, permission prompts, tool call cards, plan/thought rendering, shell output, file revision indicators, cancel/stop, conversation management, and connection state.

**Scope (this subagent produces a mockup + spec, no production code):**

1. **Create `mockup-chat-panel.html`** — a standalone HTML mockup of the right popout panel showing all ACP features in their various states. Use inline styles (this is a mockup, not production). Show:
   - Chat message area with: user messages, agent streaming text, agent thoughts (collapsed), tool call cards (read/edit/execute/search with status + diff), plan checklist, shell command cards with output, permission prompt cards with option buttons (allow once/always, reject), file revision notes, agent exited/error states, connection status banner.
   - Composer bar with: text input, send button, stop/cancel button (while running), model selector dropdown, harness selector dropdown (Claude Code / Codex / Gemini CLI / custom), harness lock toggle (pins chat to current harness, disables selector).
   - Conversation header with: conversation name (inline rename), agent + model badge, export button, delete button, rebind indicator.
   - All states: idle, running, permission-pending, cancelled, error, disconnected.

2. **Create `docs/specs/chat-panel-spec.md`** — a feature spec with a bullet list of every feature and button required, broken down by area. Keep it under 3 pages (agent context is limited). Structure:
   - **Composer bar** — every control, what it does, when it's enabled/disabled.
   - **Chat message area** — every event type rendered, what info is shown, interactive elements.
   - **Conversation header** — every control and badge.
   - **Harness & model switching** — how switching works, what happens to context, lock behavior.
   - **Permission flow** — how prompts appear, what options are shown, how responses are sent, how resolved prompts look.
   - **Connection state** — banner behavior, reconnect flow, what happens to in-flight prompts.
   - **Mobile** — how the panel adapts on mobile (bottom-nav, single panel).

   Reference `docs/acp/responsibilities.md` for the authoritative list of ACP events and features. Reference `mockup12.html` for existing UI style. Reference `web/src/components/ChatPanel.tsx` and `web/src/components/ChatMessageItem.tsx` for what's already implemented.

**Verify:** Mockup opens in a browser and renders all states. Spec is under 3 pages and covers every ACP feature from `docs/acp/responsibilities.md`.

### Work Stream 6: End-to-End Verification & Docs

**Gap:** No end-to-end runtime verification has been done. STATUS.md lists 5 unchecked runtime verification items.

**Scope (this subagent does NOT write code — it verifies and documents):**
- Run `.\build.ps1` — confirm clean build.
- Run all test gates: `go test ./...`, `go vet ./...`, `npm run build`, `npm run lint`.
- Verify `app start` serves the UI.
- Verify `app add-folder .` registers a workspace.
- Verify `app pair` generates QR + mnemonic.
- Document results in `docs/STATUS.md` under "Runtime Verification Needed" — check off what works, note what doesn't.
- Final pass on `docs/STATUS.md` — ensure every row is honest, every gap is marked, every completed item is checked.
- Update `docs/plans/OpenItems.md` — remove resolved items, note any new gaps discovered during verification.

**Verify:** All gates green + STATUS.md accurate.

## Execution Order

```
Step 1 (you):     Consolidate plans → docs/plans/execution-plan.md
Step 2a (subagent): Work Stream 1 — ACP session lifecycle (no deps)
Step 2b (subagent): Work Stream 2 — Permission policy (no deps)
Step 2c (subagent): Work Stream 3 — Agent context provider (no deps)
  ↓ (review all three, integrate)
Step 2d (subagent): Work Stream 5 — Chat panel mockup & spec (no code deps; can run parallel to 1-3)
  ↓ (review all, integrate)
Step 2e (subagent): Work Stream 4 — Open items & hardening (depends on 1-3 being merged)
  ↓
Step 2f (subagent): Work Stream 6 — E2E verification & docs (depends on all above)
```

Work streams 1, 2, 3, and 5 are independent and can run in parallel. Stream 4 depends on 1-3 being merged. Stream 6 runs last.

## Final Checklist

Before declaring done, verify:
- [ ] `go test ./...` passes
- [ ] `go vet ./...` passes
- [ ] `npm run build` passes
- [ ] `npm run lint` passes
- [ ] `.\build.ps1` passes
- [ ] `docs/STATUS.md` is accurate — no false "✅ Done" on partial work
- [ ] `docs/acp/responsibilities.md` ⚠️ markers are resolved or accurately tracked
- [ ] `docs/plans/OpenItems.md` reflects current state
- [ ] `docs/plans/execution-plan.md` exists and is accurate
- [ ] `mockup-chat-panel.html` renders all ACP panel states in a browser
- [ ] `docs/specs/chat-panel-spec.md` is under 3 pages and covers every ACP feature
