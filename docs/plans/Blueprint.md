# Local Agent Interface

## Product Design Blueprint

> **Version:** 2.7
>
> Architecture for the Local Agent Interface. ACP-only, agent-agnostic—any ACP-compatible agent works without provider-specific code.

---

# 1. Vision

Self-hosted web code editor with AI built in. A host daemon serves a browser-based IDE (VS Code-style layout, CodeMirror 6) to any device on the local network. Devices pair via QR code or four-word passcode. The application is an **ACP client** that orchestrates external agents—it does not contain AI itself.

* Background daemon + CLI on the host; all authoritative state stays on the host.
* Multiple devices (desktop, laptop, phone) connect as thin clients over LAN.
* Built-in editor for viewing, editing, and diffing files alongside agent chat.
* Any ACP-compatible agent works with no per-agent integration code.
* Explicit device pairing required before UI access.

---

## Product Summary

**First device:** `app add-folder .` registers the workspace, `app pair` generates a QR code. Scan it → paired → web UI opens. Select a model, type instructions, attach images, watch the agent work—streamed as chat while files update on the host.

**Second device:** Navigate to the local IP → lock screen. Run `app pair`, type the four-word passcode (e.g., *purple-fox-delta-wave*). State is broadcast globally, so the new device immediately shows the same chat, tasks, and file tree.

---

# 2. Core Principles

## Client Ownership

The client owns:

* Filesystem (within workspace boundaries)
* Headless shell execution on behalf of agents
* Workspace state
* Permission dialogs and approval decisions
* Session state and event history

Agents plan and propose; the client executes approved actions and returns results via ACP.

---

## Agent Independence

The UI never communicates directly with agent implementations. All interaction flows through ACP:

```
User → Browser UI → ACP Client Layer → ACP → AI Agent
```

---

## Capability Negotiation

At init, client and agent exchange capabilities via ACP (sessions, streaming, image/audio, filesystem, shell, MCP, permissions, agent modes, cancellation). The UI adapts to negotiated capabilities. See Section 7.

---

## Non-Goals (v1)

* **Interactive terminal UI** — command output appears in the tool timeline, not a shell tab
* **Public internet / remote access** — LAN-only
* **Git merge orchestration** — users run git on the host
* **Provider-specific agent APIs** — ACP is the sole integration path

Manual shell work uses the host terminal and `app` CLI.

---

## Stateless UI

The web client never owns session state. All authoritative state lives on the server; devices may disconnect and reconnect freely.

---

## Event Driven

Every meaningful action becomes an event (see Section 11). The UI renders from the event stream rather than maintaining independent state.

---

# 3. Architecture

```
Host Machine
─────────────────────────
CLI (app add-folder, app pair, ...)
Background Daemon
Workspace files (authoritative)

        │
        ▼
Local Agent Server
─────────────────────────
Device pairing & auth
Event broadcast
Workspace Manager
ACP Client Layer

        │
        ▼ (WebSocket / HTTP, LAN only)
Browser UI (any paired device)
─────────────────────────
Chat
File Tree
Permission Dialogs
Settings
Session History
Agent Selector

        │
        ▼
ACP Client Layer
─────────────────────────
Start agent
Send prompts
Receive events
Handle permissions
Stream updates
Manage sessions

        │
        ▼
ACP
        │
        ▼
AI Agent
(Claude Code, Codex CLI, Gemini CLI, Cursor CLI, OpenCode, etc.)
```

The **host daemon** manages workspaces, serves the web UI, and owns all files on disk.

The **Browser UI** renders chat, file tree, permissions, settings, and session history on any paired device. Unpaired devices see only a lock screen.

The **ACP Client Layer** handles protocol mechanics: starting agents, prompts, streaming, permissions, session lifecycle.

**Agents** handle AI reasoning, tool planning, and code generation. They do not own the filesystem or execute shell commands directly.

---

# 4. Terminology

## Workspace

A project directory managed by the client. Contains files, git repo, configuration, and sessions. Workspaces persist indefinitely.

---

## Session

One conversation with an agent over ACP. Contains conversation history, agent metadata, event log, ACP connection reference, and configuration. Sessions may be resumed via ACP (`create session`, `load session`, `list sessions`).

---

## Agent Connection

The active ACP link between the client and a running agent process. Maintained until the session closes or the agent exits.

---

## Host Daemon

Long-running background process on the host. Serves the web UI, manages workspaces, broadcasts events, and runs the ACP Client Layer. Users interact via the `app` CLI (setup) and web UI (agent sessions).

### Daemon Lifecycle

| Phase | Behavior |
|---|---|
| **Install** | Single binary added to PATH. State stored in `~/.local-agent/`. |
| **Start** | `app start` launches the daemon (foreground or `--background`). Binds to the LAN; see Network Discovery. |
| **Autostart** | Opt-in only via `app install-service` (launchd on macOS, systemd on Linux). Default is manual `app start`. |
| **Running** | One daemon per machine. Owns all workspace writes and ACP agent connections. |
| **Crash** | Daemon exits; service manager or user restarts it. Paired clients reconnect via WebSocket and resync events. Active ACP sessions resume if the agent supports it. |
| **Upgrade** | `app stop` → replace binary → `app start`. Event log and paired devices persist across restarts. |

### Host CLI

```
app — Local Agent Interface host CLI

Usage: app <command> [options]

Commands:
  start              Start the background daemon
  stop               Stop the daemon
  status             Show URL, LAN IP, port, workspaces, and paired devices
  add-folder [path]  Register a directory as a workspace (default: current dir)
  pair               Generate QR code and mnemonic passcode for device pairing
  devices            List paired devices
  revoke <id>        Revoke a paired device's access
  install-service    Install autostart (launchd / systemd; opt-in)
  logs               Tail daemon logs
  help               Show this help
```

---

## Paired Client

A browser session on a device that has completed pairing and holds a valid credential. Only paired clients access the UI beyond the lock screen. Multiple may connect simultaneously.

---

## Worker

Server-side object managing one agent connection: starts the process, maintains ACP communication, translates messages to events, handles permissions, detects restarts, supports cancellation, and cleans up.

---

# 5. Agent Registration

Each registered agent declares: name, version, supported models, ACP version, capabilities, configuration schema, authentication requirements, resume support, and launch command.

New agents can be added without changing the UI.

---

# 6. ACP Client Layer

Core integration surface implementing ACP protocol mechanics (not provider-specific):

* Process launch and transport setup
* Session management (`create`, `load`, `list`, `close`)
* Prompts (`session/prompt`) and streaming
* Permission requests (`session/request_permission`)
* Capability and authentication negotiation
* Cancellation and interrupt
* Event translation, termination detection, reconnects

Business logic belongs in the server; this layer handles protocol mechanics.

---

# 7. ACP Integration

ACP standardizes nearly everything needed for the Local Agent Interface to talk to an AI coding agent. The ACP Client Layer (Section 6) implements these mechanics.

## What ACP Handles

* **Session management** — create, load, list, close, and cancel running work.
* **Prompt exchange & streaming** — `User → session/prompt → Agent → streaming updates`; the UI renders progress (thinking, reading, editing, finished) in real time.
* **Permission requests** — agents send `session/request_permission`; the client returns *allow once*, *allow always* (per session or tool), or *reject once*. If the client never responds, the agent waits.
* **Authentication** — providers can be logged into before sessions begin.
* **Capability negotiation** — client and agent exchange supported features (images, audio, filesystem, shell, MCP, session features) at initialization.
* **Agent modes** — Ask, Plan, and Agent, switchable without protocol changes.
* **Cancellation** — interrupt a running task, like Ctrl+C.

## What ACP Does Not Handle

ACP does not define UI concerns—visual design, permission dialog appearance, file explorer/editor layout, themes, diff viewer, or chat interface. Those belong entirely to the Local Agent Interface.

---

# 8. Permission Manager

Because the client owns approval decisions, the application includes a dedicated Permission Manager.

### Responsibilities

* Receive `session/request_permission` from agents
* Present permission dialogs to the user
* Return allow-once, allow-always, or reject-once decisions to the agent
* Enforce configurable trust policies (e.g., auto-approve reads, always ask before shell commands or writes)
* Maintain an audit log of approvals and denials

### UI Features

* Allow Once
* Allow for This Session
* Always Allow This Tool
* Deny
* View full command or change details before deciding

### Policy Examples

* Auto-approve reads within workspace
* Always prompt before shell commands or file writes
* Remember decisions per session or per tool type

### Prompt Routing

Permission prompts are **broadcast to all paired devices** simultaneously. Any device may respond; the first response is applied and synchronized to all clients. This lets a user approve a shell command or file write from whichever device is in hand.

**Single-user model:** Pairing assumes one user with multiple devices. If multiple people pair to the same daemon, the first-response-wins model could let one person approve a command another is trying to deny. Multi-user support is a future concern (see OpenItems).

---

# 9. Agent Lifecycle

## Initialization

The ACP Client Layer validates configuration, starts the agent process, establishes transport, negotiates capabilities, and registers the worker.

---

## Active Operation

The worker receives prompts via `session/prompt`, streams updates to clients, handles permissions, emits events, and monitors connection health. It remains active until the agent exits or the session closes.

---

## Shutdown

Triggers: user stops session, agent exits/crashes, OS terminates the process. The worker records the event, cleans up, closes the ACP connection, updates session state, and notifies clients.

---

# 10. Session Lifecycle

Sessions are independent of agent processes. An agent may terminate while the session remains available for resumption via ACP (`load session`).

States: Created → Starting → Running → Waiting for Permission → Interrupted → Completed → Failed → Archived.

If the agent supports resume, a new process reconnects to the existing session. Otherwise, history is retained and a new session is created. Resume support is exposed via capability negotiation.

---

# 11. Event System

Every significant action is an immutable event. Types include: SessionCreated, SessionStarted, PromptSubmitted, ResponseStarted, StreamUpdate, ResponseCompleted, ToolRequested, ToolStarted, ToolCompleted, ShellCommandStarted, ShellOutputStreamed, ShellCommandCompleted, FileRevisionUpdated, PermissionRequested, PermissionGranted, PermissionDenied, SessionInterrupted, SessionCancelled, AgentExited, ConnectionRestarted, SessionResumed.

Events are appended chronologically. Application state is derived from event history—simplifying multi-client sync, debugging, and future replay.

---

# 12. Multi-Client Synchronization

The server is authoritative. Connected devices are thin clients rendering from the event stream via WebSockets.

On reconnect: session state is retrieved, missing events synced, live streaming resumes, and in-flight permission prompts are re-presented (timeout configurable, default 5 min).

Multiple devices observe and interact with the same session simultaneously. A newly paired device immediately populates with current chat, tasks, and file tree. File changes propagate via revision tracking and `FileRevisionUpdated` events.

---

# 13. Workspace Management

Registered via `app add-folder .`. Each workspace stores: project path, display name, git info, available agents, configuration, active and archived sessions.

Multiple sessions may exist per workspace. Agents can be switched between sessions. Workspace configuration is agent-independent.

---

# 14. File System Access

The host daemon owns the filesystem within workspace boundaries. Agents and browsers never write to disk directly. Agents request permission through ACP; the daemon validates, prompts the user if needed, executes, and returns results.

## Client File Sync

The host executes all file writes: agent operations through ACP, user saves from browser clients. Every file has a monotonic revision number that increments on each write. When a file changes, the host emits a `FileRevisionUpdated` event so all paired clients update their editor view live.

### User Edits

On save, the client sends content plus `expectedRevision`. If revisions match, the host applies and broadcasts. If stale, the host attempts a **three-way merge**:
- **Clean merge** — applied automatically, no user action.
- **Conflicts** — client opens a merge UI (CodeMirror 6 merge view) for resolution.

### Live Agent Changes During Editing

When the agent modifies a file being edited, the editor shows an indicator without forcing a reload. Conflict resolution triggers only on save.

**v1 limitation:** Full file content sent on every save/event. Incremental sync is a future optimization.

---

# 15. Shell Execution

The host daemon executes approved shell commands on behalf of agents via ACP—not an interactive terminal.

## Flow

1. Agent requests execution via `session/request_permission`.
2. Permission Manager prompts the user.
3. On approval, daemon runs the command in a workspace-scoped subprocess.
4. Stdout/stderr streamed to clients as `ShellOutputStreamed` events.
5. Exit code returned to agent via ACP.

Output appears in the **tool timeline** as expandable cards (command, success/failure, full output). No interactive terminal in v1. Commands run only within workspace boundaries.

---

# 16. Logging and Diagnostics

Levels: server logs, ACP Client Layer logs, ACP message logs, agent process output (diagnostics only), session event logs. Kept separate from the user experience.

---

# 17. User Interface Architecture

Responsive dashboard for desktop, tablet, and mobile. Design goals: fast, keyboard-friendly, multi-device, agent-independent, minimal clutter, accessible. Capabilities exposed dynamically based on ACP negotiation.

---

## Primary Layout

Unpaired devices see a **lock screen**. Paired devices see a VS Code-style layout:

* **Activity bar** (far left, icon-only) — file explorer, search; connection status and settings at bottom
* **Left sidebar** (popout) — workspace switcher + file tree, or search
* **Center editor** — tabbed CodeMirror 6 with diff/save and status bar
* **Right sidebar** — agent chat with harness/model selectors, chat history, streaming responses, tool timelines, permissions, input

Mobile: bottom-nav layout, one panel at a time. Layout is consistent regardless of active agent.

---

## Activity Bar and Left Sidebar

Icon-only navigation: **Files** (workspace switcher + file tree), **Search** (across workspace), **Settings** (devices, theme, config).

Left sidebar shows the selected view. File tree supports expand/collapse, unsaved-change indicators, and active file highlight. On mobile, collapses into a horizontal bar with pill buttons.

---

## Workspace View

Displays: project info, current agent, active sessions, recent activity, git branch/status, connected clients.

---

## Right Sidebar — Agent Chat

Always visible on desktop alongside the editor:

* **Harness selector** — pick agent
* **Model selector** — pick model
* **Chat history popout** — past and active sessions
* **Conversation view** — streaming responses, tool timelines, inline permissions (rendered from events)
* **Input composer** — text + attachment for multimodal uploads

On mobile, full-screen view via bottom nav.

---

## Editor and File Viewing

**CodeMirror 6** with modular extensions. Clients render from `FileRevisionUpdated` events for live updates. Supports direct editing (with `expectedRevision` saves), live viewing, and diff view (`@codemirror/merge`).

---

## Running Tasks Panel

Shows active workers: agent, model, status, runtime, current task, token usage, cancellation controls. Multiple workers may run simultaneously.

---

## Agent Management

Register/remove agents, configure auth, set default models, view capabilities, test connectivity. Settings are isolated from workspace config.

---

# 18. MCP Integration

MCP servers are managed by the client and advertised to agents during ACP capability negotiation. Each server has: name, connection status, transport, available tools, resources, and configuration.

Supports local, remote, and multiple simultaneous MCP servers, plus dynamic discovery where available.

---

# 19. Authentication

Two independent layers: **device pairing** (access to the web UI) and **agent auth** (connecting to AI providers via ACP).

---

## Device Pairing

Unpaired requests see only a lock screen.

### First Device (QR Code)

1. `app add-folder .` to register workspace.
2. `app pair` generates a QR code (URL + one-time token) and four-word mnemonic.
3. Scan QR → token submitted → device receives a long-lived credential → paired.

Pairing sessions are short-lived and single-use.

### Additional Devices (Mnemonic Passcode)

1. Navigate to daemon's local IP → lock screen.
2. `app pair` on host → read four-word mnemonic → type it into lock screen.
3. Device receives credential, UI loads synchronized.

### Device Credentials

Each device gets a unique, revocable credential stored in the browser. Required for all API/WebSocket connections. Revocable from Settings; re-pairing required after revocation or expiry.

### Security Model

No unauthenticated access to workspaces, files, or sessions. Pairing codes expire quickly and work once. See Section 22.

---

## Agent Authentication (ACP Provider Auth)

ACP handles auth negotiation before sessions begin. Methods: API keys, OAuth, local credentials, environment variables, provider-managed login.

Agent credentials must never be stored in plaintext; use OS secure credential storage where available.

---

# 20. Configuration

Three scopes, inheriting Global → Workspace → Session (each level may override).

## Global Configuration

Registered agents, network settings (bind address, port, mDNS hostname), paired devices, theme, logging, security, default models, permission policies.

### Network Discovery

The daemon binds to **`0.0.0.0`** on a configurable port (default **`7337`**) so all LAN interfaces accept connections.

| Method | How |
|---|---|
| **mDNS** | Advertise as `app.local` (Bonjour on macOS, Avahi on Linux) so browsers can use `http://app.local:7337` |
| **QR code** | `app pair` encodes the full URL (mDNS hostname or LAN IP) |
| **Manual** | `app status` prints the LAN IP and port as fallback |
| **Lock screen** | Unpaired clients can enter the host IP manually before pairing |

mDNS is recommended but not required—`app status` always provides a direct IP fallback.

---

## Workspace Configuration

Project path, preferred agent, environment variables, workspace permissions, default MCP servers.

---

## Session Configuration

Selected model, temperature, system prompt, permission mode, agent mode (Ask/Plan/Agent), context limits.

---

# 21. Error Handling

Categories: agent unavailable, auth failed, ACP communication failure, agent crash, session resume unsupported, tool execution failure, network interruption, permission denied.

Errors should provide actionable guidance. Recoverable failures should offer retry or resume.

---

# 22. Security

LAN-only with mandatory device pairing. The network is not assumed trusted—unpaired devices get nothing.

Principles: mandatory pairing, short-lived single-use pairing sessions, revocable per-device credentials, workspace boundary enforcement, explicit permission prompts, configurable trust policies, secure credential storage, audit logging, host-authoritative writes.

Safe for semi-public environments when pairing codes are treated as temporary passwords. Remote/internet access out of scope for v1.

---

# 23. Future Expansion

Potential: remote hosting, team collaboration, shared workspaces, plugin system, agent marketplace, background scheduling, mobile optimization, analytics, session replay, distributed workers, cloud sync, git merge orchestration, ACP sub-workers, additional ACP capabilities.

Designed to be additive without architectural changes.

---

# 24. Design Philosophy

ACP client and orchestration platform—not an AI platform. Owns ACP client role, workspaces, filesystem, shell, permissions, sessions, client sync, and UX. AI reasoning and tool planning belong to the agents.

---

# 25. Development Phases

## Phase 1 – Core Infrastructure

* Host daemon and CLI (`app start`, `app stop`, `app status`, `app add-folder`, `app pair`, `app devices`, `app revoke`, `app logs`)
* Device pairing (QR code + mnemonic passcode, lock screen)
* Local web server (LAN binding, mDNS discovery)
* Workspace management
* Session lifecycle
* ACP Client Layer (transport, sessions, prompts, streaming)
* Permission Manager (basic allow/deny)
* Headless shell execution (workspace-scoped subprocesses, tool timeline output)
* Single agent support
* Event system
* WebSocket synchronization across paired clients
* CodeMirror 6 editor with diff view and direct editing

---

## Phase 2 – Multi-Agent Support

* Agent registry
* Capability negotiation
* Multiple simultaneous workers
* Agent configuration and authentication
* Session resume support
* Permission policies and audit log
* Enhanced diagnostics

---

## Phase 3 – Advanced Features

* MCP management
* Multi-client collaboration
* Plugin architecture
* Improved diagnostics
* Session replay
* Advanced workspace tools
* Optional developer terminal (user-initiated manual shell, separate from agent shell execution)
* UI polish and accessibility improvements

---

# Conclusion

ACP-only, agent-agnostic foundation. The client owns workspace resources and permissions; any ACP-compatible agent works through a single integration path.
