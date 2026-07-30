# src/acp/

## Responsibility

Agent Communication Protocol (ACP) integration boundary and execution engine. Manages agent processes, profile detection, context windows, prompt streaming, and conversation state persistence.

## Module Map

- **`mod.rs`** — ACP subsystem module declaration and public export interface.
- **`core.rs`** / **`core/`** — Core ACP actor lifecycle, message dispatching, and agent session handling.
- **`autodetect/`** — Agent discovery and executable autodetection for installed harnesses (e.g. Claude, Gemini, Codex).
- **`agent_registry.rs`** — Central registry managing registered agent instances and runtime availability.
- **`context.rs`** — Context window policy, token tracking, and message truncation rules.
- **`conversation.rs`** — Active conversation turn management and event state assembly.
- **`profile.rs`** / **`profile_config.rs`** — Agent profile definitions, custom arguments, system prompts, and configuration persistence.
- **`providers.rs`** — ACP provider abstraction mapping external agent APIs to unified internal protocols.
- **`store.rs`** — Conversation history storage and session state persistence.
- **`stream.rs`** — Real-time event streaming and SSE/WebSocket delta delivery to frontend clients.

## Rules & Patterns

- **Integration Boundary**: `acp/` is the sole agent integration boundary; no direct per-agent integration allowed outside this module.
- **Asynchronous Actor Pattern**: Agent execution must run as isolated async tasks using message channels for event propagation.
- **Context Safeguards**: All outgoing context payloads must pass through `context.rs` policies to prevent token window overflow.
- **Tool Scoping**: Tool execution requests from agents are returned to the daemon for client/user permission validation.
