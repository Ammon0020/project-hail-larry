# src/api/

## Responsibility

Daemon HTTP/REST and WebSocket API route handlers serving frontend client requests.

## Module Map

```text
src/api/
├── mod.rs           router, state, shared errors/helpers
├── auth.rs          request and WebSocket auth
├── pair.rs          pairing and rate limits
├── devices.rs       device/revocation routes
├── workspaces.rs    workspace registration/trust
├── files.rs         file CRUD, search, raw serving
├── preview.rs       preview tickets/cookies/serving
├── events.rs        event history queries
├── agents.rs        agent CRUD/autodetect
├── sessions.rs      session lifecycle/prompts
├── permissions.rs   permission routes
├── embed.rs         embedded assets/SPA fallback
├── git.rs           Git routes and operations
├── mcp.rs           MCP config/status routes
├── profiles.rs      profile CRUD routes
├── providers.rs     provider routes
├── session_extra.rs uploads/context/export routes
├── settings.rs      daemon/user settings routes
└── test_support.rs  shared API test harness
```

## Rules & Patterns

- **Path Security**: All file and directory endpoints must validate paths against registered workspace boundaries; reject symlinks and path traversal attempts.
- **Error Serialization**: Use structured error responses (`ApiError`) with standard HTTP status codes; do not leak internal stack traces to unauthenticated clients.
- **Async Non-Blocking**: Long-running or IO-bound operations must offload to task pools or stream progress via WebSockets rather than blocking Axum worker threads.
