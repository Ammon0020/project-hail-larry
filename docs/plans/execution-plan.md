# Execution Plan — Finish the Codebase

> Single source of truth for the remaining work. Subagents work from their stream's section.
> Reconciled against the actual codebase on 2026-06-27. Where this doc conflicts with older
> planning docs (`docs/archive/agent-context.md`, `docs/archive/acp-stability.md`), this wins.

## Verified Current State

- `internal/acp/transport.go` — `Transport` has `Initialize`, `NewSession`, `Prompt`, `Cancel`, `Close` (kills process). **No** `LoadSession`/`DeleteSession` wrappers. `Initialize` returns `acp.InitializeResponse`, which carries `.Capabilities` — surface it to callers.
- `internal/acp/acp.go` — `startTransportLocked` always calls `NewSession`; never tries `LoadSession` even when `session.acpSessionID` is non-empty (it's cleared on rebind/restart, so load is currently impossible). `CloseSession` calls `transport.Close()` (kill) then deletes the session — no ACP `DeleteSession`. `Session` struct (`acp.go:54`) has `acpSessionID` field but it is NOT persisted (lowercase, no JSON tag).
- `internal/permissions/permissions.go` — `Request` always blocks on `respCh`. No policy map. Decisions recorded in `auditLog` only. `PermissionDecision` constants in `interfaces.go:173-176`: `allow_once`, `allow_session`, `allow_always`, `deny`. Note: ACP spec uses `reject_once`/`reject_always`; the codebase collapses these into `deny`. Policy enforcement should treat `allow_always` and `allow_session` as auto-resolve (session-scoped). Skip reject-always auto-deny for now (no constant) and document it.
- `internal/config/config.go` — `Config` has `Port`, `Host`, `DataDir`, `DBPath`, `Workspaces`, `Agents`. **No** `pairingTTL`, **no** `tlsEnabled`/cert path.
- `internal/pairing/pairing.go:115` — TTL hardcoded `5 * time.Minute`.
- `internal/server/server.go:181` — `ListenAndServe` only; no TLS path.
- `internal/workspace/workspace.go` — `Manager` has `Register` and `List` only. **No** `Remove`/`Unregister`. `WorkspaceManager` interface (`interfaces.go:99`) has no remove method.
- `cmd/app/main.go` — has `add-folder`, `pair`, `start`, `stop`, `status`, `devices`, `revoke`, `logs`. **No** `remove-folder`, **no** `list-folders`.
- `web/src/App.tsx` — `activeSessionId` IS persisted to localStorage (lines 43-61). `openTabs`, `activeTabId`, `leftPanel`, `mobileView` are NOT persisted.
- `web/src/hooks/useBackend.ts` — `activeWorkspace` is NOT persisted (only set when workspaces load and none active).
- `web/src/components/ChatPanel.tsx:52-53` — `selectedAgent`, `selectedModel` are local state, NOT persisted.
- `docs/STATUS.md` — 5 overstated rows: tasks 5, 6, 7 (⚠️ Partial), plus UI Persistence and Conversation Management unchecked.

## Out of Scope (do NOT implement)

- Device credential expiry, multi-user, image upload, ACP sub-workers, editor mobile touch optimization, session replay, developer terminal UI, MCP management, multi-client collaboration. (Per `docs/plans/OpenItems.md` lower-priority / Phase 3.)

---

## Work Stream 1 — ACP Session Lifecycle Completion  ✅ Done

> **Status: Complete (2026-06-27).** `LoadSession`/`DeleteSession` wrappers exist on `Transport` (`internal/acp/transport.go`), `ACPSessionID` is persisted in `conversations.json`, `startTransportLocked` attempts `LoadSession` when the capability is advertised and falls back to `NewSession`, `CloseSession` calls `DeleteSession` before killing the process, and `CloseAllSessions` is wired into daemon shutdown for graceful close. Tests in `internal/acp/lifecycle_test.go`. The detailed scope below is retained as a design reference.

**Original gap:** `session/load` and `session/delete` never called; shutdown killed processes instead of graceful close.

**Files:**
- `internal/acp/transport.go` — add `LoadSession(ctx, acpSessionID) (string, error)` and `DeleteSession(ctx, acpSessionID) error` wrappers on `Transport` using `t.conn.LoadSession` / `t.conn.DeleteSession` (verify exact method names against installed `coder/acp-go-sdk@v0.13.5` — use Context7 if unsure).
- `internal/acp/acp.go`:
  - Persist `acpSessionID`: add exported field `ACPSessionID string \`json:"acpSessionId,omitempty"\`` on `Session`. Update `LoadConversations`/`persistLocked` consumers (the field auto-serializes via `encoding/json`).
  - `startTransportLocked`: after `Initialize`, capture `initResp.Capabilities`. If `session.ACPSessionID != ""` AND `initResp.Capabilities.LoadSession` is true, try `transport.LoadSession(ctx, session.ACPSessionID)`; on success reuse it. On any failure (not supported, session gone), fall back to `transport.NewSession` and overwrite `session.ACPSessionID`.
  - `CloseSession`: before `transport.Close()`, if transport non-nil and `session.ACPSessionID != ""`, best-effort `transport.DeleteSession(ctx, session.ACPSessionID)` (ignore errors — process may already be dead).
  - Add `CloseAllSessions(ctx) error` method: iterate `c.sessions`, call `CloseSession` gracefully. Wire into `daemon.cleanup()` (`internal/daemon/daemon.go:236`) so SIGINT/SIGTERM triggers graceful close instead of kill.
- `internal/acp/lifecycle_test.go` (new): tests with a mock transport asserting (1) load is attempted when `ACPSessionID` set + capability true, (2) fallback to `NewSession` on load error, (3) `CloseSession` calls `DeleteSession` before kill, (4) `CloseAllSessions` closes every session.

**Acceptance:**
- Restarting the daemon and resuming a conversation reuses the agent's prior session when the agent supports `LoadSession`; falls back silently otherwise.
- `CloseSession` calls ACP `DeleteSession` before killing the process.
- Daemon shutdown closes all sessions gracefully.

**Verify:** `go test ./internal/acp/...`, `go vet ./...`

**Docs to update on completion:**
- `docs/STATUS.md` task 6 row → ✅ Done (or honest partial).
- `docs/reference/acp/responsibilities.md` line 23 — remove the `⚠️ _Planned:_ session/load … session/delete` clause; rewrite as ✅ implemented with the fallback note.

---

## Work Stream 2 — Permission Policy Enforcement  ✅ Done

> **Status: Complete (2026-06-27, with `reject_always` added 2026-07-08).** `allow_always`/`allow_session` auto-resolve from a session-scoped policy map, `reject_always` auto-denies via a deny cache mirroring the allow cache, `ClearSession` drops policies on close, and auto-resolved decisions are recorded in the audit log. Tests in `internal/permissions/permissions_test.go`. The detailed scope below is retained as a design reference.

**Original gap:** `allow_always` / `allow_session` decisions recorded but never auto-resolved; `reject_always` not cached. Every request blocked.

**Files:**
- `internal/permissions/permissions.go`:
  - Add `policy map[policyKey]interfaces.PermissionDecision` to `Manager` where `policyKey = {sessionID, toolKind, target}` (target = first affected location path, or "" if none). Initialize in `NewManager`.
  - In `Request`: before creating the pending request, check the policy map. If a matching `allow_always` / `allow_session` entry exists, record audit and return immediately without blocking or invoking `cb`. (Treat `allow_session` and `allow_always` both as auto-resolve; `allow_once` and bare `deny` are NOT auto-resolved — only an explicit `reject_always`-style decision would be, but the codebase has no such constant, so skip reject-always auto-deny for now and document it.)
  - After a blocking decision is received in `Request`, if the decision is `allow_always` or `allow_session`, write it to the policy map keyed by `(sessionID, toolKind, target)`.
  - Add `ClearSession(sessionID)` to drop policies when a session closes.
- `internal/permissions/permissions_test.go` — add tests:
  1. `allow_always` auto-resolves a subsequent same-`(session,kind,target)` request without blocking.
  2. `allow_session` auto-resolves within the same session.
  3. `allow_once` does NOT auto-resolve — second request blocks.
  4. Policy is session-scoped: a decision in session A does not affect session B.
  5. `ClearSession` removes that session's policies.
  6. Auto-resolved decisions still appear in the audit log.
- `internal/interfaces/interfaces.go` — add `ClearSession(sessionID string)` to `PermissionManager`.
- `internal/acp/acp.go` `CloseSession` — call `permMgr.ClearSession(sessionID)`.

**Acceptance:**
- A user clicking "Allow always" for a tool kind+target never gets re-prompted for that combination in the same session.
- "Allow once" still prompts every time.
- Audit log records auto-resolved decisions.

**Verify:** `go test ./internal/permissions/...`, `go vet ./...`

**Docs to update on completion:**
- `docs/STATUS.md` task 7 row → ✅ Done.
- `docs/reference/acp/responsibilities.md` line 30 — remove `⚠️ _Planned:_` prefix from the permission policy bullet.

---

## Work Stream 3 — Agent Context Provider  ✅ Done

> **Status: Complete (2026-06-27, extended 2026-07-08).** `internal/acp/context.go` + `providers.go` implement the `PromptPipeline` with `FirstPromptContextMiddleware` (workspace path, OS, file tree, git status, AGENTS.md on first prompt) plus `TimeMiddleware`, `OpenFilesMiddleware`/`OpenFilesResourceMiddleware`, and `RecentEditsMiddleware` (per-prompt). Context is sent as structured `resource` ContentBlocks when the agent advertises `embeddedContext`, with a `resource_link` + text fallback otherwise (ACP spec compliance P1.1/P1.3). Wired in `internal/daemon/daemon.go`. Tests in `internal/acp/context_test.go`. The detailed scope below is retained as a design reference.

**Original gap:** Agents got no workspace context on first prompt → excessive shell round-trips for file discovery.

**Plan source:** `docs/archive/agent-context.md` (DRAFT). Reconciled against codebase:
- `Transport.Prompt` (`transport.go:367`) sends `[]acp.ContentBlock{acp.TextBlock(content)}`. Context injection = prepend to `content` string (Option A in the draft). Simplest, universal.
- `SendPrompt` (`acp.go:240`) is the injection point — before the goroutine calls `session.transport.Prompt`.
- `WorkspaceManager.FileTree` (`interfaces.go:107`) returns `[]FileNode` (recursive). Use it directly.
- `WorkspaceManager` does NOT expose git status. Add a helper that runs `git -C <path> status --short -b` and `git -C <path> log -5 --oneline`; degrade gracefully if not a git repo or git missing. Keep this in the new file, not in `workspace/`.

**Files:**
- New `internal/acp/context.go` (keep in `acp` package to avoid an import cycle — `acp` already imports `interfaces`):
  - `type PromptMiddleware interface { BeforePrompt(ctx, *PromptContext) (action PromptAction, injected string) }`
  - `type PromptAction int` with `ActionContinue`, `ActionInject`.
  - `type PromptContext struct { SessionID, WorkspaceID, WorkspacePath, UserPrompt string; PromptCount int }`
  - `type PromptPipeline struct { middlewares []PromptMiddleware }` with `RunBeforePrompt` concatenating inject messages and bumping a per-session prompt counter.
  - `FirstPromptContextMiddleware` — injects only when `PromptCount == 0`. Builds: workspace root path, OS/platform string, flat file-path list (capped at 200 files, depth ≤ 3, grouped by top-level dir), git branch + clean/dirty summary + last 5 commits (best-effort, omitted on error), and `AGENTS.md` content if present at workspace root. Keep total context ≤ ~8KB.
  - Per-session prompt counters: store in the middleware (map[sessionID]int) keyed by session ID, reset on `Reset(sessionID)`.
- `internal/acp/acp.go`:
  - Add `pipeline *PromptPipeline` field to `Client`. Add `SetPipeline(*PromptPipeline)` setter. If nil, behavior unchanged (backward compatible).
  - In `SendPrompt`, after the lazy-restart block and before emitting `EventPromptSubmitted`, build a `PromptContext` (workspace path is already resolved in `startTransportLocked` — plumb it through `session` or re-resolve here), run the pipeline, and if it returns injected content, prepend `injected + "\n\n---\n\n" + content` for the `EventPromptSubmitted` event payload AND the `transport.Prompt` call. Increment the per-session counter after a successful inject.
- `internal/daemon/daemon.go` — construct the pipeline with `FirstPromptContextMiddleware` and call `acpClient.SetPipeline(...)` next to the existing `acpClient` wiring (~line 137).
- `internal/acp/context_test.go` — tests per the draft's testing section:
  - Pipeline: empty → continue; multiple middlewares → messages concatenated.
  - `FirstPromptContextMiddleware`: first prompt injects, second does not, `Reset(sessionID)` re-enables, empty workspace yields minimal context, large workspace truncates at 200 files / 8KB, non-git workspace omits git section gracefully, depth limit enforced.
  - Integration: mock transport receives injected content on first prompt only.

**Acceptance:**
- First prompt of a session includes workspace path, OS, file tree (≤200 files), git status, and `AGENTS.md`.
- Subsequent prompts are unmodified.
- Pipeline is nil-safe — existing behavior preserved when not wired.

**Verify:** `go test ./internal/acp/...`, `go vet ./...`

**Docs to update on completion:**
- `docs/STATUS.md` — change the "📋 Draft" agent-context line to ✅ implemented.
- `docs/archive/agent-context.md` — add a "Status: IMPLEMENTED 2026-06-27" note at the top; do not delete (keep as design reference).

---

## Work Stream 4 — Open Items & Hardening

**Depends on:** Streams 1-3 merged (touches the same files).

**Files & scope:**

**4a. TLS on LAN**
- `internal/config/config.go` — add `TLSEnabled bool \`json:"tlsEnabled"\`` and `TLSCertDir string \`json:"tlsCertDir,omitempty"\`` (default `<DataDir>/tls`). Fill defaults in `Load`.
- New `internal/server/tls.go` — `ensureSelfSignedCert(certDir, host) (certPath, keyPath, error)`: generate ECDSA P-256 self-signed cert valid for `localhost`, `127.0.0.1`, and the LAN IP(s) (use `net.Interfaces` to enumerate). 1-year validity. Trust-on-first-use: if cert exists, reuse; never overwrite.
- `internal/server/server.go` — add `ListenAndServeTLS(addr, cert, key string) error` using `httpServer.ListenAndServeTLS`. In `ListenAndServe`, branch on a new `tlsEnabled bool` field on `Server` (set via `Deps` or a setter).
- `internal/daemon/daemon.go` — read `cfg.TLSEnabled`; if true, ensure cert then call `ListenAndServeTLS`. Log `https://` instead of `http://`.

**4b. Pairing TTL configurable**
- `internal/config/config.go` — add `PairingTTLSeconds int \`json:"pairingTtlSeconds,omitempty"\`` (default 300).
- `internal/pairing/pairing.go:115` — accept a `ttl time.Duration` (add to `Manager` struct, set via `NewManager(ttl)` or a setter `SetTTL`). Replace `5 * time.Minute`.
- `internal/daemon/daemon.go` — pass `cfg.PairingTTLSeconds` into the pairing manager.

**4c. UI persistence**
- `web/src/App.tsx` — persist `openTabs`, `activeTabId`, `leftPanel`, `mobileView` to `localStorage` via `useEffect` (mirror the existing `activeSessionId` pattern at lines 43-61). Restore on init via lazy `useState` initializers.
- `web/src/hooks/useBackend.ts` — persist `activeWorkspace` (by id) to `localStorage`; on workspaces load, restore the matching workspace object.
- `web/src/components/ChatPanel.tsx` — persist `selectedAgent` and `selectedModel` to `localStorage`; restore on mount, falling back to `agents[0]` if missing.
- Use a single `localStorage` key namespace (e.g. `lai:openTabs`, `lai:activeWorkspace`, `lai:selectedAgent`, `lai:selectedModel`) to avoid collisions.

**4d. Workspace CLI gaps**
- `internal/workspace/workspace.go` — add `Remove(ctx, id) error`. Add `Remove` to `interfaces.WorkspaceManager`.
- `internal/config/config.go` — when a workspace is removed, also drop it from `cfg.Workspaces` and `Save`.
- `cmd/app/main.go` — add `newRemoveFolderCommand()` (`remove-folder <id>`) and `newListFoldersCommand()` (`list-folders`). Wire into `newRootCommand`. `remove-folder` calls the workspace manager's `Remove` and persists config. `list-folders` prints id + path + name, one per line.

**Acceptance:**
- With `tlsEnabled: true` in config, `app start` serves HTTPS with a self-signed cert generated on first run.
- `pairingTtlSeconds` in config controls QR/mnemonic expiry.
- Reload the browser → open tabs, active workspace, active session, selected agent/model all restored.
- `app list-folders` lists registered workspaces; `app remove-folder <id>` removes one.

**Verify:** `go test ./...`, `go vet ./...`, `npm run build`, `npm run lint`, `.\build.ps1`

**Docs to update on completion:**
- `docs/STATUS.md` — task 5 row → ✅ Done; check off UI Persistence row.
- `docs/plans/OpenItems.md` — check off TLS on LAN, Pairing TTL, UI persistence; leave Device credential expiry, Editor on mobile, ACP sub-workers unchecked.

---

## Work Stream 5 — Chat Panel Mockup & Feature Spec

**No code deps; can run parallel to 1-3.** Produces a mockup + spec only.

**Files:**
- `mockup-chat-panel.html` (repo root) — standalone HTML with inline styles showing every ACP chat panel state. Cover:
  - Message area: user message, agent streaming text with cursor, agent thought (collapsed `<details>`), tool call cards (read/edit/execute/search with status + diff), plan checklist, shell command card with output, permission prompt card with option buttons (allow once/always, reject), file revision note, agent exited/error state, connection status banner (connected/reconnecting/disconnected).
  - Composer bar: text input, send button, stop/cancel button (visible while running), model selector dropdown, harness selector dropdown (Claude Code / Codex / Gemini CLI / custom), harness lock toggle (pins chat to current harness — disables selector).
  - Conversation header: inline-renameable name, agent + model badge, export button, delete button, rebind indicator.
  - All states: idle, running, permission-pending, cancelled, error, disconnected.
- `docs/specs/chat-panel-spec.md` — feature spec, under 3 pages, bulleted by area:
  - Composer bar — every control, what it does, enabled/disabled rules.
  - Chat message area — every event type rendered, info shown, interactive elements.
  - Conversation header — every control and badge.
  - Harness & model switching — how switching works, what happens to context, lock behavior.
  - Permission flow — how prompts appear, options shown, how responses are sent, resolved-prompt appearance.
  - Connection state — banner behavior, reconnect flow, in-flight prompt handling.
  - Mobile — bottom-nav single-panel adaptation.

**References:** `docs/reference/acp/responsibilities.md` (authoritative ACP events/features), `mockup12.html` (existing UI style), `web/src/components/ChatPanel.tsx` + `ChatMessageItem.tsx` (what's already implemented).

**Acceptance:**
- Mockup opens in a browser and renders all states legibly.
- Spec is under 3 pages and covers every ACP feature from `responsibilities.md`.

**Verify:** Open `mockup-chat-panel.html` in a browser; confirm all states render. Read spec end-to-end.

---

## Work Stream 6 — End-to-End Verification & Docs

**Depends on:** all above. This subagent does NOT write code — it verifies and documents.

**Steps:**
- Run `.\build.ps1` — confirm clean build.
- Run all gates: `go test ./...`, `go vet ./...`, `npm run build`, `npm run lint`. Record results.
- Verify `app start` serves the UI (start daemon, curl `http://localhost:7337/health`, then stop).
- Verify `app add-folder .` registers a workspace (run, then `app list-folders`).
- Verify `app pair` generates a QR + mnemonic (capture stdout, confirm both present).
- Document results in `docs/STATUS.md` under "Runtime Verification Needed" — check off what works, note what doesn't with a one-line reason.
- Final pass on `docs/STATUS.md` — every row honest, every gap marked, every completed item checked. No false "✅ Done".
- Update `docs/plans/OpenItems.md` — remove resolved items, note any new gaps discovered.
- Confirm `docs/plans/execution-plan.md` (this file) reflects final state — mark each stream ✅ at completion.

**Verify:** All gates green + STATUS.md accurate + OpenItems.md accurate.

---

## Execution Order

```
Step 1 (done):     This document written.
Step 2a (done):    Stream 1 — ACP session lifecycle      ✅
Step 2b (done):    Stream 2 — Permission policy          ✅
Step 2c (done):    Stream 3 — Agent context provider     ✅
Step 2d (done):    Stream 5 — Chat panel mockup & spec   ✅
  ↓ review + integrate all four
Step 2e (done):    Stream 4 — Open items & hardening     ✅
  ↓
Step 2f (done):    Stream 6 — E2E verification & docs    ✅
```

## Final Checklist

- [x] `go test ./...` passes
- [x] `go vet ./...` passes
- [x] `npm run build` passes
- [x] `npm run lint` passes
- [x] `.\build.ps1` passes
- [x] `docs/STATUS.md` accurate — no false "✅ Done" on partial work
- [x] `docs/reference/acp/responsibilities.md` ⚠️ markers resolved or accurately tracked
- [x] `docs/plans/OpenItems.md` reflects current state
- [x] `docs/plans/execution-plan.md` exists and is accurate
- [x] `mockup-chat-panel.html` renders all ACP panel states in a browser
- [x] `docs/specs/chat-panel-spec.md` under 3 pages, covers every ACP feature

## Completion Status

All six work streams are complete and merged (2026-06-27). All gates green. Runtime verification confirmed: `app start` serves the UI (`/health` 200, root div present), `app add-folder .` + `app list-folders` register and list workspaces, `app pair` emits a mnemonic passcode + QR code + token URL. The only remaining verification gap is ACP transport end-to-end (requires a real agent process) — tracked in `docs/plans/OpenItems.md`.
