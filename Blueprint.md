# Local Agent Interface

## Product Design Blueprint

> **Version:** 2.6
>
> This document defines the architecture for the Local Agent Interface application. It supersedes earlier drafts while preserving all existing functionality. The design is ACP-only, agent-agnostic, and intended to support any ACP-compatible agent without provider-specific integration code.

---

# 1. Vision

The Local Agent Interface is a powerful, self-hosted web interface that transforms CLI-based coding agents (like Claude Code and Cursor CLI) into collaborative, multi-device assistants. It moves the agent out of a rigid terminal window and into an intelligent, event-driven web dashboard.

Rather than acting as an AI itself, the application implements an **ACP client** that orchestrates one or more external agents through the Agent Client Protocol. A lightweight **host daemon** runs on the user's machine, serving the web UI to any device on the local network while keeping files on the host computer perfectly synced and safe from overwrite collisions.

The application is designed to:

* Run entirely on a personal computer via a background daemon and CLI.
* Serve multiple devices (desktop, laptop, phone) over the local network as thin clients, with all authoritative state on the host.
* Provide a built-in code editor so users can view, edit, and diff files directly alongside the agent chat.
* Connect to any ACP-compatible agent through ACP alone, with no per-agent integration code.
* Require explicit device pairing before any web client may access the UI.

---

## Product Summary

**Setup and first device:** The user runs `app add-folder .` in a project directory to register the workspace, then `app pair` to generate a QR code. Scanning it with a phone pairs the device and opens the web UI, where they select a model, type instructions, attach images, and watch the agent write code, run shell commands, and spawn sub-workers—rendered as a chat stream while files are modified on the host.

**Adding a second device:** On a laptop, the user navigates to the app's local IP and is met with a lock screen. They run `app pair` again, read the four-word mnemonic passcode (e.g., *purple-fox-delta-wave*), and type it in. Because state is broadcast globally, the laptop immediately shows the same active chat, agent tasks, and file tree as the phone.

---

# 2. Core Principles

## ACP Only

ACP is the only communication protocol between the Local Agent Interface and agents.

The application does not implement provider-specific APIs, terminal scraping, process piping, or other bespoke integration mechanisms. If an agent does not expose ACP, it is not supported until it does.

---

## Client Ownership

The Local Agent Interface is the ACP client. It owns:

* The filesystem (within workspace boundaries)
* Shell execution on behalf of agents (headless; no terminal UI)
* The workspace
* Permission dialogs and approval decisions
* Session state and event history

Agents plan work, request tools, and propose changes. The client executes approved actions and returns results through ACP.

---

## Agent Independence

The UI never communicates directly with Claude Code, Codex, Gemini CLI, Cursor CLI, OpenCode, or any other agent implementation.

Instead:

```
User
  │
  ▼
Browser UI
  │
  ▼
ACP Client Layer
  │
  ▼
ACP
  │
  ▼
AI Agent
```

The client does not need to know whether the backend is Claude, Codex, Gemini, or another agent—as long as it speaks ACP.

---

## Capability Negotiation

During initialization, the client and agent exchange capabilities through ACP (sessions, streaming, image/audio input, filesystem access, headless shell execution, MCP servers, permissions, agent modes, cancellation). The UI adapts automatically to negotiated capabilities instead of hardcoding agent behavior. See Section 7 for details.

---

## Non-Goals (v1)

The following are explicitly out of scope for the initial release:

* **Interactive terminal UI panel** — no xterm-style shell tab in the web interface; command output appears in the session tool timeline instead
* **Public internet / remote access** — LAN-only; paired devices on the local network only
* **Git merge orchestration** — branch integration, pull, rebase (users run git on the host)
* **Provider-specific agent APIs** — ACP is the sole integration path
* **Terminal multiplexing or stdout/stderr scraping** — no attaching to external terminal sessions

Users who need a manual shell for setup use the host machine's real terminal and the `app` CLI.

---

## Stateless UI

The web client never owns session state. All authoritative state lives on the server, so any device may disconnect and reconnect without affecting running agents.

---

## Event Driven

Every meaningful action becomes an event (prompts, stream updates, tool and shell activity, permission decisions, session and agent lifecycle, device pairing—see Section 11). The UI renders from the event stream instead of maintaining its own independent state.

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

The **host daemon** runs on the user's machine, manages workspaces, serves the web UI to the local network, and owns all files on disk.

The **Browser UI** presents the user experience on any paired device: chat, file tree, permission dialogs, settings, and session history. Unpaired devices see only a lock screen.

The **ACP Client Layer** implements the protocol: starting agents, sending prompts, receiving streaming updates, handling `session/request_permission`, and managing session lifecycle.

**Agents** handle AI reasoning, tool planning, and code generation. They request capabilities from the client; they do not own the filesystem or execute shell commands directly.

In short, the **server** owns sessions, routing, device pairing/auth, event storage and broadcast, workspace management, the agent registry, and permission policies. The **agent** owns AI reasoning, tool planning, and model behavior. The Local Agent Interface owns orchestration, workspace resources, and user approval—not intelligence.

---

# 4. Terminology

## Workspace

A workspace represents a project directory managed by the client.

A workspace contains:

* Files
* Git repository
* Configuration
* Multiple sessions

Workspaces persist indefinitely.

---

## Session

A session represents one conversation with an agent over ACP.

A session contains:

* Conversation history
* Agent metadata
* Event log
* ACP connection reference
* Configuration

Sessions may be resumed via ACP (`create session`, `load session`, `list sessions`).

---

## Agent Connection

An agent connection is the active ACP link between the client and a running agent process.

The client starts the agent binary, establishes the ACP transport, and maintains the connection until the session closes or the agent exits.

---

## Host Daemon

The host daemon is the long-running background process on the user's machine. It serves the web UI, manages workspaces, broadcasts events to paired clients, and runs the ACP Client Layer. Users interact with it primarily through the `app` CLI for setup tasks (registering folders, pairing devices) and through the web UI for agent sessions.

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

A paired client is a browser session on a device (phone, laptop, tablet) that has completed device pairing and holds a valid authentication credential. Only paired clients may access the web UI beyond the lock screen. Multiple paired clients may connect simultaneously and receive the same live event stream.

---

## Worker

A worker is the server-side object responsible for managing one agent connection.

Responsibilities include:

* Starting agent processes
* Establishing and maintaining ACP communication
* Translating ACP messages into internal events
* Handling `session/request_permission` on behalf of the user
* Restart detection
* Cancellation (`cancel running work`)
* Cleanup

One worker manages one agent connection.

---

# 5. Agent Registration

Each registered agent declares:

* Name
* Version
* Supported models
* ACP version
* Capabilities
* Configuration schema
* Authentication requirements
* Resume support
* Launch command

The registry allows new agents to be added without changing the UI, as long as they expose ACP.

---

# 6. ACP Client Layer

The ACP Client Layer is the core integration surface. It is not a provider-specific adapter; it implements the ACP protocol on behalf of the application.

Responsibilities include:

* Launching agent processes
* Establishing ACP transport
* Session management (`create`, `load`, `list`, `close`)
* Sending prompts (`session/prompt`)
* Receiving streaming updates
* Handling permission requests (`session/request_permission`)
* Capability negotiation during initialization
* Authentication negotiation
* Cancellation and interrupt
* Translating ACP messages into internal events
* Detecting agent termination
* Managing reconnects

Business logic belongs in the server; the ACP Client Layer handles protocol mechanics.

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

ACP does not define UI concerns—visual design, permission dialog appearance, file explorer/editor layout, themes, diff viewer, or chat interface. Those belong entirely to the Local Agent Interface. Earlier integration approaches (direct CLI integration, terminal multiplexing, stdout/stderr parsing, provider-specific APIs) are out of scope; ACP is the single integration path.

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

* Auto-approve file reads within the workspace
* Always prompt before shell commands
* Always prompt before file writes
* Remember decisions per session or per tool type

Every ACP-compatible agent uses the same permission flow. The client does not implement separate approval logic per agent.

### Prompt Routing

Permission prompts are **broadcast to all paired devices** simultaneously. Any device may respond; the first response is applied and synchronized to all clients. This lets a user approve a shell command or file write from whichever device is in hand.

---

# 9. Agent Lifecycle

Each agent connection follows the same lifecycle over ACP.

## Initialization

The Workspace Manager requests an agent connection.

The ACP Client Layer:

1. Validates configuration.
2. Starts the agent process.
3. Establishes ACP transport.
4. Negotiates capabilities.
5. Registers the active worker.

---

## Active Operation

While connected, the worker:

* Receives user prompts via `session/prompt`
* Streams agent updates to clients
* Handles permission requests
* Emits events
* Tracks connection health
* Monitors agent process state

The worker remains active until the agent exits or the session is explicitly closed.

---

## Shutdown

Shutdown may occur because:

* The user stops the session.
* The agent exits normally.
* The agent crashes.
* The operating system terminates the process.

Regardless of the cause, the worker:

* Records the termination event.
* Cleans up resources.
* Closes the ACP connection.
* Updates session state.
* Notifies connected clients.

---

# 10. Session Lifecycle

Sessions are independent of agent processes.

An agent process may terminate while a session remains available for later resumption via ACP (`load session`).

A session progresses through states such as:

* Created
* Starting
* Running
* Waiting for Permission
* Interrupted
* Completed
* Failed
* Archived

If the agent supports session resume, a new process may reconnect to an existing session through ACP.

If resume is unsupported, the application retains the history while creating a new agent session when appropriate.

Resume behavior is exposed through capability negotiation rather than assumed by the application.

---

# 11. Event System

Every significant action is represented as an immutable event.

Example event types include:

* SessionCreated
* SessionStarted
* PromptSubmitted
* ResponseStarted
* StreamUpdate
* ResponseCompleted
* ToolRequested
* ToolStarted
* ToolCompleted
* ShellCommandStarted
* ShellOutputStreamed
* ShellCommandCompleted
* FileRevisionUpdated
* PermissionRequested
* PermissionGranted
* PermissionDenied
* SessionInterrupted
* SessionCancelled
* AgentExited
* ConnectionRestarted
* SessionResumed

Events are appended to the session log in chronological order, and application state is derived from this history rather than maintained as an independent source of truth—simplifying multi-client synchronization, debugging, and future replay.

---

# 12. Multi-Client Synchronization

The server is the authoritative source of state. All files live on the host machine; connected devices are thin clients that render from the global event stream.

Clients subscribe to events using WebSockets.

When a client reconnects:

1. The current session state is retrieved.
2. Missing events are synchronized.
3. Live streaming resumes automatically.

Multiple paired devices may observe and interact with the same running session simultaneously without interrupting the underlying agent connection. A newly paired device immediately populates with the current chat history, active tasks, and file tree—no manual refresh or handoff. File changes propagate via revision tracking and `FileRevisionUpdated` events (see File System Access).

---

# 13. Workspace Management

A workspace represents a project directory managed by the server on the host machine.

Workspaces are registered via the host CLI (e.g., `app add-folder .` run inside the project directory).

Each workspace stores:

* Project path
* Display name
* Git information
* Available agents
* Configuration
* Active sessions
* Archived sessions

Multiple sessions may exist within a single workspace, and agents may be switched between sessions without affecting the workspace. Workspace configuration remains agent-independent.

---

# 14. File System Access

The host daemon owns the filesystem within workspace boundaries. Agents and browsers never write to disk directly.

When an agent needs to read or modify files, it requests permission through ACP. The daemon validates the request, presents it to the user if required, performs the operation, and returns the result to the agent.

The server is responsible for:

* Validating workspace boundaries
* Preventing access outside approved directories
* Executing approved file operations on behalf of agents
* Recording file-related events

This keeps filesystem control with the host and maintains a clear audit trail, as ACP intends.

## Client File Sync

The host executes all file writes: agent operations through ACP, user saves from browser clients. Every file has a monotonic revision number that increments on each write. When a file changes, the host emits a `FileRevisionUpdated` event so all paired clients update their editor view live.

### User Edits

Users edit from any paired device. On save, the client sends content plus `expectedRevision` (the revision when editing began).

1. **Revision matches** — host applies the save, increments revision, broadcasts to all clients.
2. **Revision stale** — the file changed since editing began (e.g., the agent edited it). The host attempts a **three-way merge** using the user's base revision, the user's new content, and the current file content:
   - **Merge succeeds without conflicts** — host applies the merged result, increments revision, and broadcasts to all clients. No user intervention needed.
   - **Merge produces conflicts** — host returns both versions and the client opens a merge UI (Monaco diff editor, side-by-side) so the user can resolve conflicts and save again.

### Live Agent Changes During User Editing

When the agent modifies a file the user is actively editing, the editor shows an indicator (e.g., "File modified by agent") without forcing a reload of the user's unsaved buffer. The user may review incoming changes at any time. The conflict resolution above only triggers on save.

Git operations (branch merges, pull, rebase) are out of scope for v1. Users run git on the host directly.

---

# 15. Shell Execution

The host daemon executes approved shell commands on behalf of agents. This is an ACP backend capability—not an interactive terminal in the web UI.

## Flow

1. Agent requests command execution via `session/request_permission` through ACP.
2. The Permission Manager prompts the user on a paired device.
3. On approval, the daemon runs the command in a **workspace-scoped subprocess** on the host.
4. Stdout and stderr are streamed to all paired clients as events (`ShellOutputStreamed`).
5. Exit code and final results are returned to the agent through ACP.

## UI Presentation

Command output appears in the **session tool timeline** as expandable cards—not in a terminal pane. Users see what the agent ran, a summary (success/failure), and can expand to read full output.

There is no interactive terminal panel in the web UI for v1. Users who need a manual shell use the host machine's terminal or the `app` CLI for setup and administration.

## Constraints

* Commands run only within approved workspace boundaries.
* The application does not attach to external terminal sessions or multiplexers.
* All shell I/O flows through ACP permission requests and the host daemon's subprocess runner.

---

# 16. Logging and Diagnostics

Logging exists at multiple levels:

* Server logs
* ACP Client Layer logs
* ACP message logs
* Agent process output (for diagnostics only—not used as a communication channel)
* Session event logs

Logs should support troubleshooting via developer tooling, kept separate from the primary user experience and without exposing unnecessary internal details.

---

# 17. User Interface Architecture

The interface is designed as a responsive dashboard that functions well on desktop, tablet, and mobile devices.

## Design Goals

* Fast and responsive
* Keyboard-friendly
* Multi-device capable
* Agent-independent
* Minimal visual clutter
* Accessible and extensible

The UI should expose capabilities dynamically based on negotiated ACP capabilities rather than assuming support for specific features.

---

## Primary Layout

Unpaired devices see a **lock screen** prompting for a mnemonic passcode or QR scan.

Paired devices see the full application:

* Top navigation bar
* Left navigation drawer (collapsible with hamburger menu)
* Main workspace area
* Right-side optional context panel
* Bottom status bar

The layout should remain consistent regardless of the active agent.

---

## Left Navigation

The navigation drawer provides quick access to:

* Workspaces
* Sessions
* Running Agents
* Agent Registry
* MCP Servers
* Permission Policies
* Settings (including paired device management)
* Logs
* Diagnostics

Future features should be added here without requiring structural redesign.

---

## Workspace View

Each workspace displays:

* Project information
* Current agent
* Active sessions
* Recent activity
* Git branch and status (when available)
* Connected clients

Users may create multiple sessions within the same workspace.

---

## Session View

The primary session interface contains:

* Conversation history
* Streaming responses
* Tool execution timeline (including expandable shell command output)
* Permission requests
* Input composer
* Session metadata
* Agent mode selector (when supported)

The conversation view renders directly from the event stream.

---

## Editor and File Viewing

The main editor uses **Monaco**. Clients render file content from `FileRevisionUpdated` events so the view stays live while files change.

* **Direct editing** — users edit files from any paired device. Saves go to the host with `expectedRevision` (see Client File Sync).
* **Live viewing** — all clients follow file changes in real time via the event stream.
* **Diff view** — Monaco's built-in diff editor for reviewing changes.

---

## Running Tasks Panel

A dedicated panel displays currently active work.

Each running worker displays:

* Agent
* Model
* Current status
* Runtime
* Current task
* Token usage (when available)
* Cancellation controls

Multiple workers may execute simultaneously.

---

## Agent Management

Users may:

* Register agents
* Remove agents
* Configure authentication
* Select default models
* View negotiated capabilities
* Test ACP connectivity

Agent settings are isolated from workspace configuration.

---

# 18. MCP Integration

Model Context Protocol (MCP) servers are managed by the client and advertised to agents during ACP capability negotiation.

Each MCP server includes:

* Name
* Connection status
* Transport
* Available tools
* Resource list
* Configuration

Agents that support MCP receive configured servers through ACP during session initialization.

The architecture supports:

* Local MCP servers
* Remote MCP servers
* Multiple simultaneous MCP servers
* Dynamic discovery where supported

---

# 19. Authentication

Authentication is split into two independent layers: **application authentication** (pairing a device to access the web UI) and **agent authentication** (connecting to an AI provider via ACP).

---

## Application Authentication (Device Pairing)

Before any browser may access the web UI, the device must be paired with the host daemon. Unpaired requests see only a secure lock screen.

### First Device (QR Code)

1. User registers a workspace on the host: `app add-folder .`
2. User initiates pairing: `app pair`
3. The daemon generates a **pairing session** containing:
   * A QR code encoding the daemon's local URL and a one-time pairing token
   * A human-readable **four-word mnemonic passcode** (e.g., *purple-fox-delta-wave*)
4. User scans the QR code with their phone camera.
5. The phone opens the web UI, submits the token, and receives a long-lived **device credential**.
6. The phone is paired. The full UI loads immediately.

Pairing sessions are short-lived and single-use. Once consumed, the QR code and passcode are invalidated.

### Additional Devices (Mnemonic Passcode)

1. User opens a browser on the new device and navigates to the daemon's local IP address (e.g., `http://192.168.1.50:8080`).
2. The browser displays the lock screen.
3. User runs `app pair` again on the host terminal.
4. User reads the four-word mnemonic from the host screen and types it into the lock screen on the new device.
5. The new device receives a device credential and the full UI loads, synchronized with all other active clients.

### Device Credentials

* Each paired device receives a unique, revocable credential stored securely in the browser.
* Credentials are required for all API and WebSocket connections.
* Users may revoke paired devices from Settings on any authenticated client.
* Re-pairing is required after credential revocation or expiry.

### Security Model

The daemon binds to the local network interface and exposes no unauthenticated access to workspaces, files, or sessions. Pairing codes expire quickly, work only once, and must not be shared; without a valid code the lock screen reveals nothing. See Section 22 for the full security model.

---

## Agent Authentication (ACP Provider Auth)

ACP supports authentication negotiation between the host daemon and an agent before sessions begin.

Authentication methods may include:

* API keys
* OAuth
* Local credentials
* Environment variables
* Provider-managed login

The ACP Client Layer manages agent authentication flows while presenting a common interface to the server.

Agent credentials should never be stored in plaintext.

Where supported by the operating system, secure credential storage should be used on the host machine.

---

# 20. Configuration

Configuration is organized into separate scopes.

## Global Configuration

Examples include:

* Registered agents
* Network settings (bind address, port, mDNS hostname)
* Paired devices
* Theme
* Logging
* Security
* Default models
* Permission policies

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

Examples include:

* Project path
* Preferred agent
* Environment variables
* Workspace permissions
* Default MCP servers

---

## Session Configuration

Examples include:

* Selected model
* Temperature
* System prompt
* Permission mode
* Agent mode (Ask, Plan, Agent)
* Context limits

Configuration inheritance should follow:

Global → Workspace → Session

Each level may override values from the previous level.

---

# 21. Error Handling

Errors should be categorized consistently.

Examples include:

* Agent unavailable
* Authentication failed
* ACP communication failure
* Agent crash
* Session resume unsupported
* Tool execution failure
* Network interruption
* Permission denied

Errors should provide actionable guidance whenever possible rather than exposing raw implementation details.

Recoverable failures should offer retry or resume options.

---

# 22. Security

The Local Agent Interface is designed for local network use with mandatory device pairing. It does not assume the network is fully trusted—unpaired devices cannot access any application data.

Security principles include:

* Mandatory device pairing before UI access (QR code or mnemonic passcode)
* Short-lived, single-use pairing sessions
* Revocable per-device credentials
* No unrestricted filesystem access
* Workspace boundary enforcement
* Explicit permission prompts for agent actions
* Configurable trust policies
* Secure credential storage on the host
* Audit logging for permissions, pairing events, and administrative actions
* Host-authoritative file writes to prevent overwrite collisions across clients

The pairing model is intentionally safe for use in semi-public environments (coffee shops, co-working spaces) when users treat pairing codes like temporary passwords. Remote access over the public internet is out of scope for v1; if introduced later, transport encryption and stronger authentication should be added.

---

# 23. Future Expansion

The architecture is intentionally designed to accommodate future capabilities without major refactoring.

Potential future enhancements include:

* Remote hosting
* Team collaboration
* Shared workspaces
* Plugin system
* Agent marketplace
* Background scheduling
* Mobile-optimized interface
* Advanced analytics
* Session replay
* Distributed workers
* Cloud synchronization (optional)
* Git merge orchestration (branch integration, pull, rebase)
* ACP sub-worker support (deferred until next ACP release)
* Additional ACP capabilities as the protocol evolves

These features should be additive rather than requiring architectural changes.

---

# 24. Design Philosophy

The Local Agent Interface is an ACP client and orchestration platform—not an AI platform.

Its responsibilities are to:

* Implement the ACP client role
* Manage workspaces, filesystem, and headless shell execution
* Handle permission requests uniformly across all agents
* Maintain session state
* Synchronize clients
* Present a unified user experience

AI reasoning, model behavior, and tool planning remain the responsibility of the underlying agents.

This separation of concerns keeps the application maintainable, extensible, and resilient as both agents and ACP continue to evolve.

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
* Monaco editor with diff view and direct editing

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

This architecture establishes an ACP-only, agent-agnostic foundation for the Local Agent Interface. By implementing a proper ACP client that owns workspace resources and permission decisions, the system works with any ACP-compatible agent through a single integration path—delivering a consistent experience across devices, agents, and workflows.
