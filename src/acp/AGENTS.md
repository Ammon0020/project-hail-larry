# src/acp/

## Responsibility

Agent Communication Protocol (ACP) integration boundary and execution engine. Manages agent processes, profile detection, context windows, prompt streaming, and conversation state persistence.

## Module Map

```text
src/acp/
├── mod.rs                 exports/boundary
├── core.rs                client facade
├── core/                  actor runtime and lifecycle
│   ├── actor/              session actor/turns
│   ├── handlers/           filesystem, terminal, permission callbacks
│   ├── lifecycle/          startup/restore/teardown
│   └── registry.rs         live/dormant sessions
├── autodetect/             harness discovery
├── agent_registry.rs       registered agents
├── context.rs              context/token policy
├── conversation.rs         conversation turns/events
├── profile.rs              profile definitions
├── profile_config.rs       persisted profile config
├── providers.rs            ACP providers
├── store.rs                history/session storage
└── stream.rs               event streaming
```

## Rules & Patterns

- **Integration Boundary**: `acp/` is the sole agent integration boundary; no direct per-agent integration allowed outside this module.
- **Asynchronous Actor Pattern**: Agent execution must run as isolated async tasks using message channels for event propagation.
- **Context Safeguards**: All outgoing context payloads must pass through `context.rs` policies to prevent token window overflow.
- **Tool Scoping**: Tool execution requests from agents are returned to the daemon for client/user permission validation.
