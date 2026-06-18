# Local Agent Interface

## Product Design Blueprint

> **Version:** 2.0
>
> This document defines the architecture for the Local Agent Interface application. It supersedes earlier drafts while preserving all existing functionality. The design is ACP-first, provider-agnostic, and intended to support multiple AI providers, local models, and future expansion without architectural changes.

---

# 1. Vision

The Local Agent Interface is a locally hosted web application that provides a single interface for interacting with AI coding agents.

Rather than acting as an AI itself, the application orchestrates one or more external agent providers through a common abstraction layer. The user interacts only with the Local Agent Interface while adapters translate requests to the underlying provider.

The application is designed to:

* Run entirely on a personal computer.
* Serve multiple devices over the local network.
* Support multiple simultaneous users.
* Allow multiple agent providers.
* Remain functional even as providers evolve.
* Prefer ACP wherever possible instead of provider-specific APIs.

---

# 2. Core Principles

## ACP First

Whenever ACP supports a capability, ACP is the source of truth.

Provider-specific APIs are only used when ACP does not yet expose equivalent functionality.

Whenever ACP gains support for a feature currently implemented with provider APIs, the ACP implementation replaces the custom implementation.

---

## Provider Independence

The UI never communicates directly with Codex, Claude Code, Gemini CLI, or other providers.

Instead:

UI

↓

Application Core

↓

Execution Adapter

↓

Provider

Only adapters understand provider-specific behavior.

---

## Capability Discovery

Providers advertise supported capabilities.

Examples:

* Session creation
* Resume session
* File editing
* Streaming responses
* Tool execution
* Image input
* MCP support
* Approval workflow
* Interrupt
* Cancellation
* Background execution

The UI adapts automatically based on discovered capabilities instead of hardcoding provider behavior.

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
* Stream token received
* Tool started
* Tool completed
* Approval requested
* Approval granted
* Approval denied
* Session resumed
* Worker started
* Worker exited

The UI renders from the event stream instead of maintaining its own independent state.

---

# 3. Architecture

(Client)

↓

(WebSocket / HTTP)

↓

(Local Agent Server)

↓

Workspace Manager

↓

Execution Adapter

↓

ACP

↓

Provider

The server owns:

* Sessions
* Routing
* Authentication
* Event storage
* Workspace management
* Provider registry

The provider owns:

* AI reasoning
* Tool execution
* Token generation
* Model behavior

The Local Agent Interface owns orchestration—not intelligence.

---

# 4. Terminology

## Workspace

A workspace represents a project directory.

A workspace contains:

* Files
* Git repository
* Configuration
* Multiple sessions

Workspaces persist indefinitely.

---

## Session

A session represents one conversation with an agent.

A session contains:

* Conversation history
* Provider metadata
* Event log
* Running process reference
* Configuration

Sessions may be resumed if supported by the provider.

---

## Process

A process is the operating system process executing an agent.

Examples:

* codex
* claude
* gemini

Processes may terminate independently of sessions.

---

## Worker

A worker is the server-side object responsible for managing one process.

Responsibilities include:

* Starting providers
* Monitoring stdout/stderr
* Managing ACP communication
* Restart detection
* Cancellation
* Cleanup

One worker manages one process.

---

# 5. Provider Registration

Each provider registers:

* Name
* Version
* Supported models
* ACP version
* Capabilities
* Configuration schema
* Authentication requirements
* Resume support
* Execution mode

The registry allows providers to be added without changing the UI.

---

# 6. Execution Adapters

Execution adapters isolate provider-specific behavior from the rest of the application.

Their responsibilities include:

* Launching provider processes
* Connecting through ACP when available
* Translating ACP events into internal events
* Falling back to provider-specific APIs when ACP does not support a required capability
* Detecting provider termination
* Reporting capabilities
* Managing authentication
* Handling reconnects
* Providing a consistent interface to the Workspace Manager

Execution adapters should be as thin as possible. Business logic belongs in the server, not the adapter.

---

# 7. ACP Integration

ACP is the preferred communication protocol for all providers.

Whenever ACP exposes a capability, the application must use ACP rather than implementing provider-specific logic.

ACP is responsible for:

* Session creation
* Prompt submission
* Streaming responses
* Tool invocation
* Tool results
* Interrupt requests
* Cancellation
* Session metadata
* Capability advertisement

Provider-specific APIs are reserved for capabilities that ACP does not yet expose.

The architecture should make replacing provider-specific implementations with ACP implementations straightforward as ACP evolves.

---

# 8. Provider Lifecycle

Each provider follows the same lifecycle.

## Initialization

The Workspace Manager requests an execution adapter.

The adapter:

1. Validates configuration.
2. Starts the provider process.
3. Connects through ACP (or provider API if necessary).
4. Discovers provider capabilities.
5. Registers the active worker.

---

## Active Operation

While running, the worker:

* Receives user prompts
* Streams responses
* Executes tools
* Emits events
* Tracks health
* Monitors process state

The worker remains active until the provider exits or is explicitly stopped.

---

## Shutdown

Shutdown may occur because:

* The user stops the session.
* The provider exits normally.
* The provider crashes.
* The operating system terminates the process.

Regardless of the cause, the worker:

* Records the termination event.
* Cleans up resources.
* Closes ACP connections.
* Updates session state.
* Notifies connected clients.

---

# 9. Session Lifecycle

Sessions are independent of processes.

A process may terminate while a session remains available for later resumption.

A session progresses through states such as:

* Created
* Starting
* Running
* Waiting for Approval
* Interrupted
* Completed
* Failed
* Archived

If the provider supports session resume, a new process may reconnect to an existing session.

If resume is unsupported, the application retains the history while creating a new provider session when appropriate.

Resume behavior is therefore provider-dependent and exposed through capability discovery rather than assumed by the application.

---

# 10. Event System

Every significant action is represented as an immutable event.

Example event types include:

* SessionCreated
* SessionStarted
* PromptSubmitted
* ResponseStarted
* ResponseToken
* ResponseCompleted
* ToolRequested
* ToolStarted
* ToolCompleted
* ApprovalRequested
* ApprovalGranted
* ApprovalDenied
* SessionInterrupted
* SessionCancelled
* ProviderExited
* WorkerRestarted
* SessionResumed

Events are appended to the session log in chronological order.

The current application state is derived from the event history rather than maintained as an independent source of truth.

This approach simplifies synchronization across multiple connected clients and improves debugging and future replay capabilities.

---

# 11. Multi-Client Synchronization

The server is the authoritative source of state.

Clients subscribe to events using WebSockets.

When a client reconnects:

1. The current session state is retrieved.
2. Missing events are synchronized.
3. Live streaming resumes automatically.

Multiple devices may observe the same running session simultaneously.

A user may begin a session on one device and continue monitoring or interacting from another without interrupting the underlying provider process.

---

# 12. Workspace Management

A workspace represents a project directory managed by the server.

Each workspace stores:

* Project path
* Display name
* Git information
* Available providers
* Configuration
* Active sessions
* Archived sessions

Multiple sessions may exist within a single workspace.

Providers may be switched between sessions without affecting the workspace itself.

Workspace configuration should remain provider-independent wherever possible.

---

# 13. File System Access

The application never edits files directly unless explicitly instructed.

Instead, file modifications are performed through the active provider whenever practical.

The server is responsible for:

* Validating workspace boundaries
* Preventing access outside approved directories
* Enforcing permissions
* Recording file-related events

This maintains a clear audit trail while allowing providers to perform edits using their native capabilities.

---

# 14. Logging and Diagnostics

Logging exists at multiple levels:

* Server logs
* Adapter logs
* ACP communication logs
* Provider stdout/stderr
* Session event logs

Logs should support troubleshooting without exposing unnecessary internal implementation details to end users.

Diagnostic information should be accessible through developer tooling but separated from the primary user experience.

---

# 15. User Interface Architecture

The interface is designed as a responsive dashboard that functions well on desktop, tablet, and mobile devices.

## Design Goals

* Fast and responsive
* Keyboard-friendly
* Multi-device capable
* Provider-independent
* Minimal visual clutter
* Accessible and extensible

The UI should expose capabilities dynamically based on the connected provider rather than assuming support for specific features.

---

## Primary Layout

The application consists of:

* Top navigation bar
* Left navigation drawer (collapsible with hamburger menu)
* Main workspace area
* Right-side optional context panel
* Bottom status bar

The layout should remain consistent regardless of the active provider.

---

## Left Navigation

The navigation drawer provides quick access to:

* Workspaces
* Sessions
* Running Agents
* Providers
* MCP Servers
* Settings
* Logs
* Diagnostics

Future features should be added here without requiring structural redesign.

---

## Workspace View

Each workspace displays:

* Project information
* Current provider
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
* Approval requests
* Input composer
* Session metadata

The conversation view renders directly from the event stream.

---

## Running Tasks Panel

A dedicated panel displays currently active work.

Each running worker displays:

* Provider
* Model
* Current status
* Runtime
* Current task
* Token usage (when available)
* Cancellation controls

Multiple workers may execute simultaneously.

---

## Provider Management

Users may:

* Register providers
* Remove providers
* Configure authentication
* Select default models
* View capabilities
* Test connectivity

Provider settings are isolated from workspace configuration wherever possible.

---

# 16. MCP Integration

Model Context Protocol (MCP) servers are managed independently of providers.

Each MCP server includes:

* Name
* Connection status
* Transport
* Available tools
* Resource list
* Configuration

Providers that support MCP automatically receive configured servers during session initialization.

The architecture should support:

* Local MCP servers
* Remote MCP servers
* Multiple simultaneous MCP servers
* Dynamic discovery where supported

---

# 17. Authentication

Authentication requirements vary by provider.

Execution adapters manage provider-specific authentication mechanisms while presenting a common interface to the server.

Authentication methods may include:

* API keys
* OAuth
* Local credentials
* Environment variables
* Provider-managed login

Credentials should never be stored in plaintext.

Where supported by the operating system, secure credential storage should be used.

---

# 18. Configuration

Configuration is organized into separate scopes.

## Global Configuration

Examples include:

* Registered providers
* Network settings
* Theme
* Logging
* Security
* Default models

---

## Workspace Configuration

Examples include:

* Project path
* Preferred provider
* Environment variables
* Workspace permissions
* Default MCP servers

---

## Session Configuration

Examples include:

* Selected model
* Temperature
* System prompt
* Approval mode
* Context limits

Configuration inheritance should follow:

Global → Workspace → Session

Each level may override values from the previous level.

---

# 19. Error Handling

Errors should be categorized consistently.

Examples include:

* Provider unavailable
* Authentication failed
* ACP communication failure
* Provider crash
* Session resume unsupported
* Tool execution failure
* Network interruption
* Permission denied

Errors should provide actionable guidance whenever possible rather than exposing raw implementation details.

Recoverable failures should offer retry or resume options.

---

# 20. Security

The Local Agent Interface is designed primarily for trusted local networks.

Security principles include:

* No unrestricted filesystem access
* Workspace boundary enforcement
* Explicit provider permissions
* Secure credential storage
* Authenticated client connections
* Audit logging for administrative actions

If remote access is later introduced, authentication and transport security should be expanded accordingly.

---

# 21. Future Expansion

The architecture is intentionally designed to accommodate future capabilities without major refactoring.

Potential future enhancements include:

* Remote hosting
* Team collaboration
* Shared workspaces
* Plugin system
* Provider marketplace
* Background scheduling
* Mobile-optimized interface
* Advanced analytics
* Session replay
* Distributed workers
* Cloud synchronization (optional)
* Additional ACP capabilities as the protocol evolves

These features should be additive rather than requiring architectural changes.

---

# 22. Design Philosophy

The Local Agent Interface is an orchestration platform rather than an AI platform.

Its responsibilities are to:

* Manage workspaces
* Coordinate providers
* Maintain session state
* Synchronize clients
* Present a unified user experience
* Abstract provider differences
* Prefer open standards such as ACP whenever practical

AI reasoning, model behavior, and tool execution remain the responsibility of the underlying providers.

This separation of concerns keeps the application maintainable, extensible, and resilient as both providers and ACP continue to evolve.

---

# 23. Development Phases

## Phase 1 – Core Infrastructure

* Local web server
* Workspace management
* Session lifecycle
* ACP integration
* Execution adapter framework
* Single provider support
* Event system
* WebSocket synchronization

---

## Phase 2 – Multi-Provider Support

* Provider registry
* Capability discovery
* Multiple simultaneous workers
* Provider configuration
* Session resume support
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

This architecture establishes a provider-agnostic, ACP-first foundation for the Local Agent Interface. By clearly separating orchestration, provider integration, and user interface responsibilities, the system remains adaptable to future AI providers and protocol enhancements while delivering a consistent experience across devices and workflows.
