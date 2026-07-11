# ACP Client Is a God Struct (1023 Lines, 8+ Responsibilities)

## Location
- [acp.go](file:///media/adam/extex/projects/project-hail-larry/internal/acp/acp.go) — `Client` struct and methods (1023 lines)

## Problem

`acp.Client` is the largest file in the backend at **1023 lines** and concentrates at least 8 distinct responsibilities behind a single mutex:

1. **Agent registration** — `RegisterAgent`, `RemoveAgent`, `ListAgents`
2. **Session CRUD** — `CreateSession`, `CloseSession`, `ListSessions`, `GetSession`, `GetSessionInfo`
3. **Session naming** — `RenameSession`
4. **Session rebinding** — `RebindSession` (agent switch with transcript export)
5. **Model switching** — `SwitchModel` (ACP config option vs. rebind fallback)
6. **Prompt dispatch** — `SendPrompt` (transport lazily started, middleware pipeline, attachment translation)
7. **Persistence** — `persistLocked` / `LoadConversations` / `SetStorePath`
8. **Lifecycle wiring** — `SetCallbacks`, `SetPipeline`, `SetEventStore`, `SetConversationTransfer`, `SetMcpConfigPath`, `CloseAllSessions`

All of this shares `c.mu`, and many methods acquire the lock, do I/O (filesystem, process spawn, ACP SDK calls), and release it in the same scope — making the lock contention surface very large.

## Impact

- **Maintainability:** Any change to session management risks breaking agent registration, persistence, or prompt dispatch because they share state and locking.
- **Testability:** Tests must construct the full Client with all dependencies wired (pipeline, event store, transfer middleware, MCP config path) just to test one concern.
- **Readability:** New contributors must understand the entire 1000-line file to safely modify any part.

## Suggested Fix

Decompose into focused structs behind the same `Client` facade:

| Sub-struct | Responsibility |
|---|---|
| `AgentRegistry` | Register/remove/list agents, autodetect merge |
| `SessionStore` | CRUD, rename, list, persistence (conversations.json) |
| `SessionOrchestrator` | Rebind, switch-model, send-prompt (uses Registry + Store) |
| `TransportManager` | Lazy transport startup, close, transport pool |

Each gets its own mutex scope. `Client` becomes a thin coordinator that delegates.
