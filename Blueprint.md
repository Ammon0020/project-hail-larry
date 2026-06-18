# Local Agent Interface

## Product Design Blueprint

> **Version:** 2.1
>
> This document defines the architecture for the Local Agent Interface application. It supersedes earlier drafts while preserving all existing functionality. The design is ACP-only, agent-agnostic, and intended to support any ACP-compatible agent without provider-specific integration code.

---

# 1. Vision

The Local Agent Interface is a locally hosted web application that provides a single interface for interacting with AI coding agents.

Rather than acting as an AI itself, the application implements an **ACP client** that orchestrates one or more external agents through the Agent Client Protocol. The user interacts only with the Local Agent Interface; all agent communication flows through ACP.

The application is designed to:

* Run entirely on a personal computer.
* Serve multiple devices over the local network.
* Support multiple simultaneous users.
* Connect to any ACP-compatible agent.
* Remain functional as agents evolve, without custom per-agent integration.
* Use ACP as the sole communication protocol between the application and agents.

---

# 2. Core Principles

## ACP Only

ACP is the only communication protocol between the Local Agent Interface and agents.

The application does not implement provider-specific APIs, terminal scraping, process piping, or other bespoke integration mechanisms. If an agent does not expose ACP, it is not supported until it does.

---

## Client Ownership

The Local Agent Interface is the ACP client. It owns:

* The filesystem (within workspace boundaries)
* The terminal
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

During initialization, the client and agent exchange capabilities through ACP.

Examples:

* Session creation and resume
* Prompt exchange and streaming
* Image and audio input
* Filesystem access
* Terminal access
* MCP servers
* Permission requests
* Agent modes (Ask, Plan, Agent)
* Cancellation

The UI adapts automatically based on negotiated capabilities instead of hardcoding agent behavior.

---

## Stateless UI

The web client never owns session state.

All authoritative state lives on the server.

Any device may disconnect and reconnect without affecting running agents.

---

## Event Driven

Every meaningful action becomes an event.

Examples include:

* Prompt submitted
* Stream update received
* Tool started
* Tool completed
* Permission requested
* Permission granted
* Permission denied
* Session resumed
* Agent connected
* Agent disconnected

The UI renders from the event stream instead of maintaining its own independent state.

---

# 3. Architecture

```
Browser UI
─────────────────────────
Chat
File Tree
Terminal
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

The **Browser UI** presents the user experience: chat, file tree, terminal, permission dialogs, settings, and session history.

The **ACP Client Layer** implements the protocol: starting agents, sending prompts, receiving streaming updates, handling `session/request_permission`, and managing session lifecycle.

**Agents** handle AI reasoning, tool planning, and code generation. They request capabilities from the client; they do not own the filesystem or terminal directly.

The server owns:

* Sessions
* Routing
* Authentication
* Event storage
* Workspace management
* Agent registry
* Permission policies

The agent owns:

* AI reasoning
* Tool planning
* Token generation
* Model behavior

The Local Agent Interface owns orchestration, workspace resources, and user approval—not intelligence.

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

ACP standardizes nearly everything needed for the Local Agent Interface to talk to an AI coding agent.

## What ACP Handles

### Session Management

* Create session
* Load session
* List sessions
* Close session
* Cancel running work

### Prompt Exchange

```
User → session/prompt → Agent → streaming updates
```

### Streaming Updates

While the agent is working, it streams progress back (thinking, reading files, editing, finished). The UI updates in real time from these events.

### Permission Requests

When an agent wants to do something requiring approval (execute a shell command, modify files, etc.), it sends `session/request_permission`. The client shows a dialog, waits for the user's decision, and returns one of:

* Allow once
* Allow always (for this session or tool, per policy)
* Reject once

If the client never responds, the agent waits.

### Authentication

ACP supports authentication negotiation so the client can log into providers before sessions begin.

### Capability Negotiation

During initialization, client and agent exchange supported capabilities: images, audio, filesystem access, terminal access, MCP servers, session features, and more.

### Agent Modes

Many ACP implementations expose modes such as Ask, Plan, and Agent. The client can switch between them without changing the protocol.

### Cancellation

The client can interrupt a running task, equivalent to pressing Ctrl+C in many CLIs.

---

## What ACP Does Not Handle

ACP intentionally does not define:

* UI design
* Permission dialog appearance
* File explorer layout
* Terminal emulator implementation
* Editor layout
* Themes
* Diff viewer
* Chat interface

Those belong entirely to the Local Agent Interface (the client application).

---

## Replacing Custom Integration

Earlier architectural approaches—direct CLI integration, terminal multiplexing, stdout/stderr parsing, or provider-specific APIs—are out of scope. ACP is the single integration path.

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
* PermissionRequested
* PermissionGranted
* PermissionDenied
* SessionInterrupted
* SessionCancelled
* AgentExited
* ConnectionRestarted
* SessionResumed

Events are appended to the session log in chronological order.

The current application state is derived from the event history rather than maintained as an independent source of truth.

This approach simplifies synchronization across multiple connected clients and improves debugging and future replay capabilities.

---

# 12. Multi-Client Synchronization

The server is the authoritative source of state.

Clients subscribe to events using WebSockets.

When a client reconnects:

1. The current session state is retrieved.
2. Missing events are synchronized.
3. Live streaming resumes automatically.

Multiple devices may observe the same running session simultaneously.

A user may begin a session on one device and continue monitoring or interacting from another without interrupting the underlying agent connection.

---

# 13. Workspace Management

A workspace represents a project directory managed by the server.

Each workspace stores:

* Project path
* Display name
* Git information
* Available agents
* Configuration
* Active sessions
* Archived sessions

Multiple sessions may exist within a single workspace.

Agents may be switched between sessions without affecting the workspace itself.

Workspace configuration remains agent-independent.

---

# 14. File System Access

The client owns the filesystem within workspace boundaries.

When an agent needs to read or modify files, it requests permission through ACP. The client validates the request, presents it to the user if required, performs the operation, and returns the result to the agent.

The server is responsible for:

* Validating workspace boundaries
* Preventing access outside approved directories
* Executing approved file operations on behalf of agents
* Recording file-related events

This maintains a clear audit trail and keeps filesystem control with the client, as ACP intends.

---

# 15. Terminal Access

The client owns the terminal.

When an agent requests shell execution, it sends a permission request through ACP. The client runs approved commands in its terminal environment and returns stdout, stderr, and exit codes to the agent.

The application does not attach to external terminal sessions or multiplexers. Terminal I/O flows through ACP and the client's terminal implementation.

---

# 16. Logging and Diagnostics

Logging exists at multiple levels:

* Server logs
* ACP Client Layer logs
* ACP message logs
* Agent process output (for diagnostics only—not used as a communication channel)
* Session event logs

Logs should support troubleshooting without exposing unnecessary internal implementation details to end users.

Diagnostic information should be accessible through developer tooling but separated from the primary user experience.

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

The application consists of:

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
* Settings
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
* Tool execution timeline
* Permission requests
* Input composer
* Session metadata
* Agent mode selector (when supported)

The conversation view renders directly from the event stream.

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

ACP supports authentication negotiation between client and agent before sessions begin.

Authentication methods may include:

* API keys
* OAuth
* Local credentials
* Environment variables
* Provider-managed login

The ACP Client Layer manages authentication flows while presenting a common interface to the server.

Credentials should never be stored in plaintext.

Where supported by the operating system, secure credential storage should be used.

---

# 20. Configuration

Configuration is organized into separate scopes.

## Global Configuration

Examples include:

* Registered agents
* Network settings
* Theme
* Logging
* Security
* Default models
* Permission policies

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

The Local Agent Interface is designed primarily for trusted local networks.

Security principles include:

* No unrestricted filesystem access
* Workspace boundary enforcement
* Explicit permission prompts for agent actions
* Configurable trust policies
* Secure credential storage
* Authenticated client connections
* Audit logging for permissions and administrative actions

If remote access is later introduced, authentication and transport security should be expanded accordingly.

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
* Additional ACP capabilities as the protocol evolves

These features should be additive rather than requiring architectural changes.

---

# 24. Design Philosophy

The Local Agent Interface is an ACP client and orchestration platform—not an AI platform.

Its responsibilities are to:

* Implement the ACP client role
* Manage workspaces, filesystem, and terminal
* Handle permission requests uniformly across all agents
* Maintain session state
* Synchronize clients
* Present a unified user experience

AI reasoning, model behavior, and tool planning remain the responsibility of the underlying agents.

This separation of concerns keeps the application maintainable, extensible, and resilient as both agents and ACP continue to evolve.

---

# 25. Development Phases

## Phase 1 – Core Infrastructure

* Local web server
* Workspace management
* Session lifecycle
* ACP Client Layer (transport, sessions, prompts, streaming)
* Permission Manager (basic allow/deny)
* Single agent support
* Event system
* WebSocket synchronization

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
* UI polish and accessibility improvements

---

# Conclusion

This architecture establishes an ACP-only, agent-agnostic foundation for the Local Agent Interface. By implementing a proper ACP client that owns workspace resources and permission decisions, the system works with any ACP-compatible agent through a single integration path—delivering a consistent experience across devices, agents, and workflows.
