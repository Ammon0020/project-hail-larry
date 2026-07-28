# ACP Core Crate Extraction

## Purpose & Vision
The goal of this epic is to extract the Agent-Client Protocol (`acp`) orchestration subsystem from the `local_agent` daemon into a standalone, reusable Rust crate (`acp-core`). 

### What the Library Will Be
`acp-core` will be a generic, embeddable **AI Agent Orchestration Engine** for Rust. It will provide the core runtime for managing agent sessions, parsing the Agent-Client Protocol, running Model Context Protocol (MCP) servers, and orchestrating tool calls (like reading/writing files or running terminal commands). 

### What the Library Will Do
Any Rust application that imports `acp-core` will immediately gain the ability to:
- Discover and initialize AI agents (like Claude Code, Cursor, Devin, etc.).
- Start agent sessions attached to specific local workspaces.
- Stream prompts to the agent and receive real-time streamed responses.
- Safely execute the agent's tool-call requests (file modifications, shell commands) via injected abstractions.
- Integrate with standard MCP (Model Context Protocol) servers.

### Why We Are Doing This
Currently, the `acp` subsystem is tightly coupled to the `local_agent` daemon's specific web architecture—meaning it expects a SQLite WAL event bus, daemon-specific configuration files, and a specific terminal executor. 

By extracting this into a library and abstracting the environment behind traits (`ShellExecutor`, `EventSink`, `WorkspaceManager`, `PermissionManager`), we can embed this powerful AI agent capability into completely different architectures. Example use cases for `acp-core` include:
- **Desktop/Native Applications**: A local markdown notes app built with Tauri could embed `acp-core` to provide an AI writing assistant directly in its native desktop window, without needing to run a web daemon.
- **Headless CLI Tools**: A lightweight terminal tool (e.g. `acp-cli`) that spins up an agent in the current directory and streams output to stdout.
- **CI/CD Bots**: A GitHub Action or CI worker that spins up an agent to review PRs, run tests, and push fixes, implementing the traits to use the CI environment instead of a local filesystem.
- **Cloud/Multi-tenant Backends**: A server that orchestrates agents for multiple users over gRPC or WebSockets, implementing the `EventSink` trait against a distributed database instead of a local SQLite WAL.

## Scope
Decouple the `src/acp` module from `local_agent`'s internal event bus (`SharedEventBus`), configuration files (`crate::config`), and terminal execution (`crate::shell`), using generic traits. Move the resulting agnostic code into `crates/acp-core`.

## Dependencies
None.

## Acceptance Criteria
- `src/acp` has zero dependencies on `crate::shell`, `crate::config`, or `crate::events`.
- Traits `ShellExecutor`, `EventSink`, `WorkspaceManager`, and `PermissionManager` are cleanly defined and injected via `HandlerDeps`/`ClientDeps`.
- `src/acp` is successfully moved to its own Cargo crate (`crates/acp-core`).

## Verification
- `cargo test --all-targets` compiles and passes.
- `make check` passes (including frontend contract tests).
- Daemon starts and successfully communicates with `mockagent`, verifying the daemon successfully implemented and injected the new traits.
