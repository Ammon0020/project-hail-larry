# Local Agent Interface — Product Design Blueprint

> **Version:** 2.8 — design source of truth. ACP-only, agent-agnostic: any ACP-compatible agent works with no provider-specific code. Kept < 200 lines; status lives in `docs/STATUS.md`.

## 1. Vision

Self-hosted web code editor with AI built in. A host daemon serves a browser-based IDE (VS Code-style layout, CodeMirror 6) to any device on the local network. The app is an **ACP client** that orchestrates external agents — it contains no AI itself.

- Background daemon + CLI on the host; all authoritative state stays on the host.
- Multiple devices (desktop, laptop, phone) connect as thin clients over LAN.
- Built-in editor for viewing, editing, and diffing files alongside agent chat.
- Explicit device pairing required before UI access.

**Flow:** `app add-folder .` registers a workspace, `app pair` shows a QR + four-word passcode. First device scans the QR; additional devices open the LAN IP and type the passcode. State is broadcast, so every paired device sees the same chat, tasks, and file tree in real time.

## 2. Core Principles

- **Client ownership** — the client owns the filesystem (within workspace bounds), headless shell execution, workspace/session state, permission decisions, and event history. Agents plan and propose; the client executes approved actions and returns results via ACP.
- **Agent independence** — the UI never talks to agents directly. Everything flows `User -> Browser UI -> ACP Client Layer -> ACP -> Agent`.
- **Capability negotiation** — at init, client and agent exchange supported features (sessions, streaming, image/audio, filesystem, shell, MCP, permissions, modes, cancellation). The UI adapts to what was negotiated.
- **Stateless UI** — the web client never owns session state; authoritative state is server-side, so devices disconnect/reconnect freely.
- **Event-driven** — every meaningful action is an immutable event (§11); the UI renders from the event stream rather than independent state.
- **Non-goals (v1):** interactive terminal UI, public-internet/remote access (LAN-only), git merge orchestration, provider-specific agent APIs.

## 3. Architecture

- **Host daemon** — manages workspaces, serves the web UI, owns all files on disk, broadcasts events, runs the ACP Client Layer. Users drive it via the `app` CLI (setup) and web UI (sessions).
- **Browser UI** (any paired device) — chat, file tree, permissions, settings, session history. Unpaired devices see only a lock screen. Connects over WebSocket/HTTP, LAN only.
- **ACP Client Layer** — protocol mechanics: start agents, send prompts, stream updates, handle permissions, manage session lifecycle. Business logic stays in the server.
- **Agents** (Claude Code, Codex CLI, Gemini CLI, Cursor CLI, OpenCode, …) — AI reasoning, tool planning, code generation. They never own the filesystem or run shell commands directly.

## 4. Terminology & Daemon

- **Workspace** — a managed project directory (files, git, config, sessions); persists indefinitely.
- **Session** — one conversation with an agent over ACP (history, agent metadata, event log, ACP connection ref, config). Resumable via ACP `create`/`load`/`list session`.
- **Agent connection** — the active ACP link to a running agent process, until the session closes or the agent exits.
- **Worker** — server-side object managing one agent connection: process start, ACP comms, message->event translation, permissions, restart detection, cancellation, cleanup.
- **Paired client** — a browser holding a valid device credential; multiple may connect at once.
- **Host daemon** — one per machine; owns all workspace writes and ACP connections. State in `~/.local-agent/`.

**Daemon lifecycle:** Install (single binary on PATH) -> Start (`app start`, foreground or `--background`, binds LAN) -> Autostart (opt-in `app install-service`: launchd/systemd/HKCU) -> Running -> Crash (clients reconnect + resync; ACP sessions resume if supported) -> Upgrade (`app stop` -> replace binary -> `app start`; event log + devices persist).

**CLI:** `start`, `stop`, `status`, `add-folder [path]`, `remove-folder`, `list-folders`, `pair`, `devices`, `revoke <id>`, `install-service`/`uninstall-service`, `logs`, `help`.

## 5. Agent Registration

Each registered agent declares: name, version, supported models, ACP version, capabilities, config schema, auth requirements, resume support, and launch command. New agents are added without UI changes (autodetect probes known CLIs).

## 6–7. ACP Client Layer & Integration

The ACP Client Layer implements protocol mechanics (not provider-specific): process launch + transport, session management (create/load/list/close), prompts + streaming, permission requests, capability + auth negotiation, cancellation, and event translation / termination detection / reconnects.

**ACP handles:** session management; prompt exchange & streaming (rendered as thinking/reading/editing/finished); permission requests (`session/request_permission` -> allow-once / allow-always per session or tool / reject-once; no response => agent waits); provider auth; capability negotiation; agent modes (Ask/Plan/Agent); cancellation.

**ACP does not handle:** any UI concern — visual design, dialog appearance, explorer/editor layout, themes, diff viewer, chat. Those belong entirely to this app.

## 8. Permission Manager

Owns approval decisions. Receives `session/request_permission`, presents dialogs, returns allow-once / allow-for-session / always-allow-tool / deny, enforces configurable trust policies (e.g. auto-approve workspace reads, always prompt before shell/writes), and keeps an audit log.

**Prompt routing:** prompts are **broadcast to all paired devices**; the first response wins and syncs to all clients. **Single-user model** — pairing assumes one user with multiple devices; multi-user is a future concern (see OpenItems).

## 9–10. Agent & Session Lifecycle

- **Agent:** Init (validate config, start process, transport, negotiate caps, register worker) -> Active (prompts, streaming, permissions, events, health monitoring) -> Shutdown (user stop / agent exit / OS kill -> record event, cleanup, close ACP, update state, notify clients).
- **Session:** independent of agent processes. States: Created -> Starting -> Running -> Waiting for Permission -> Interrupted -> Completed -> Failed -> Archived. If the agent supports resume, a new process reconnects to the session; otherwise history is retained and a new session is created.

## 11. Event System

Every significant action is an immutable, chronologically appended event; app state is derived from history (simplifies multi-client sync, debugging, future replay). Types include: SessionCreated/Started, PromptSubmitted, ResponseStarted, StreamUpdate, ResponseCompleted, ToolRequested/Started/Completed, ShellCommandStarted/OutputStreamed/Completed, FileRevisionUpdated, PermissionRequested/Granted/Denied, SessionInterrupted/Cancelled, AgentExited, ConnectionRestarted, SessionResumed (plus model-change, file-changed-on-disk, and pending-action events).

## 12. Multi-Client Synchronization

The server is authoritative; devices are thin clients rendering from the event stream over WebSockets. On reconnect: retrieve session state, sync missing events, resume live streaming, re-present in-flight permission prompts (timeout default 5 min). A newly paired device immediately populates with current chat, tasks, and file tree. File changes propagate via revision tracking + `FileRevisionUpdated`.

## 13. Workspace Management

Registered via `app add-folder .`. Each workspace stores project path, display name, git info, available agents, config, and active/archived sessions. Multiple sessions per workspace; agents switchable between sessions; workspace config is agent-independent.

## 14. File System Access

The host owns all writes within workspace bounds; agents and browsers never write to disk directly. Every file has a monotonic revision that increments on each write; a `FileRevisionUpdated` event updates all clients live.

- **User edits:** save sends content + `expectedRevision`. Match => apply + broadcast. Stale => **three-way merge**: clean merges apply automatically; conflicts open a CodeMirror 6 merge view.
- **Live agent changes while editing:** the editor shows an indicator without forcing a reload; conflict resolution triggers only on save.
- **v1 limitation:** full file content sent on every save/event; incremental sync is future work.

## 15. Shell Execution

The host runs approved shell commands on behalf of agents (not an interactive terminal): agent requests via `session/request_permission` -> user prompt -> workspace-scoped subprocess -> stdout/stderr streamed as `ShellOutputStreamed` -> exit code returned via ACP. Output appears in the **tool timeline** as expandable cards. Commands run only within workspace boundaries.

## 16. Logging & Diagnostics

Separate streams kept out of the user experience: server logs, ACP Client Layer logs, ACP message logs, agent process output (diagnostics), and session event logs.

## 17. User Interface Architecture

Responsive dashboard (desktop/tablet/mobile); fast, keyboard-friendly, multi-device, agent-independent, minimal, accessible. Capabilities exposed dynamically from ACP negotiation.

- **Layout:** unpaired => lock screen. Paired => VS Code-style: activity bar (icon-only: Files, Search; connection status + Settings at bottom), left sidebar (workspace switcher + file tree or search), center tabbed CodeMirror 6 editor (diff/save + status bar), right sidebar agent chat. Mobile: bottom-nav, one panel at a time.
- **Chat (right sidebar):** harness selector, model selector, history popout, conversation view (streaming, tool timelines, inline permissions — all from events), input composer (text + attachments). Settings render as an editor tab.
- **Editor:** CodeMirror 6 with modular extensions; renders from `FileRevisionUpdated`; direct editing (`expectedRevision` saves), live viewing, diff view (`@codemirror/merge`).
- **Running tasks panel:** active workers with agent/model/status/runtime/task/tokens + cancel.
- **Agent management:** register/remove agents, configure auth, set default models, view caps, test connectivity — isolated from workspace config.

## 18. MCP Integration

MCP servers are managed by the client and advertised to agents during ACP negotiation (name, status, transport, tools, resources, config). Config is a Claude-compatible `mcpServers` map at `~/.local-agent/mcp.json`; supports local/remote and multiple simultaneous servers. Today the client passes inline stdio/http/sse transports to the agent (capability-filtered); daemon-brokered MCP-over-ACP is blocked on the SDK (see `acp-spec-compliance.md` §4.10).

## 19. Authentication

Two independent layers:

- **Device pairing** (web-UI access): unpaired => lock screen. First device — `app add-folder .`, then `app pair` shows a QR (URL + one-time token) + four-word mnemonic; scan => credential => paired. Additional devices — open the LAN IP, type the mnemonic. Credentials are unique, revocable, browser-stored, required on all API/WS connections; pairing codes are short-lived and single-use.
- **Agent auth** (ACP provider auth): API keys, OAuth, local credentials, env vars, or provider-managed login, negotiated before sessions. Never store credentials in plaintext — use OS secure storage where available.

## 20. Configuration

Three inheriting scopes (Global -> Workspace -> Session; each may override):

- **Global** — registered agents, network settings, paired devices, theme, logging, security, default models, permission policies.
- **Workspace** — project path, preferred agent, env vars, workspace permissions, default MCP servers.
- **Session** — model, temperature, system prompt, permission mode, agent mode, context limits.

**Network discovery:** daemon binds `0.0.0.0` on a configurable port (default `7337` HTTP / `7338` HTTPS). Discovery via mDNS (`app.local`), QR (full URL), `app status` (LAN IP fallback), or manual lock-screen IP entry.

## 21. Error Handling

Categories: agent unavailable, auth failed, ACP comms failure, agent crash, resume unsupported, tool execution failure, network interruption, permission denied. Errors give actionable guidance; recoverable failures offer retry or resume.

## 22. Security

LAN-only with mandatory device pairing; the network is not assumed trusted (unpaired devices get nothing). Principles: mandatory pairing, short-lived single-use pairing sessions, revocable per-device credentials, workspace-boundary enforcement, explicit permission prompts, configurable trust policies, secure credential storage, audit logging, host-authoritative writes. Safe for semi-public environments when pairing codes are treated as temporary passwords. See `docs/known-issues.md` for deferred security findings.

## 23–24. Future Expansion & Philosophy

Additive without architectural change: remote hosting, team collaboration, shared workspaces, plugins, agent marketplace, background scheduling, session replay, distributed workers, cloud sync, git merge orchestration, ACP sub-workers, more ACP capabilities. This is an **ACP client and orchestration platform, not an AI platform** — AI reasoning and tool planning belong to the agents.

## 25. Development Phases

- **Phase 1 — Core Infrastructure (done):** daemon + CLI, pairing (QR + mnemonic + lock screen), LAN web server + mDNS, workspace management, session lifecycle, ACP Client Layer, Permission Manager, headless shell execution, single-agent support, event system, WebSocket sync, CodeMirror 6 editor with diff.
- **Phase 2 — Multi-Agent (partial):** agent registry, capability negotiation, multiple simultaneous workers, agent config/auth, session resume, permission policies + audit log, enhanced diagnostics.
- **Phase 3 — Advanced (partial):** MCP management, multi-client collaboration, plugin architecture, session replay, advanced workspace tools, optional developer terminal, UI polish + accessibility.
