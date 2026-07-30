# src/api/

## Responsibility

Daemon HTTP/REST and WebSocket API route handlers serving frontend client requests.

## Module Map

- **`mod.rs`** — Axum web router setup, API endpoint registrations, authentication middleware, and error handling.
- **`embed.rs`** — Static web asset embedding and SPA fallback handler.
- **`git.rs`** — Git workspace operations, diff generation, branch control, and staging endpoints.
- **`mcp.rs`** — Model Context Protocol (MCP) server status and configuration endpoints.
- **`profiles.rs`** — Agent profile management endpoints (CRUD for agent configurations and options).
- **`providers.rs`** — Available LLM/ACP provider listing and status endpoints.
- **`session_extra.rs`** — Extended session operations, file uploads, and auxiliary context attachments.
- **`settings.rs`** — User preference and daemon configuration update endpoints.

## Rules & Patterns

- **Path Security**: All file and directory endpoints must validate paths against registered workspace boundaries; reject symlinks and path traversal attempts.
- **Error Serialization**: Use structured error responses (`ApiError`) with standard HTTP status codes; do not leak internal stack traces to unauthenticated clients.
- **Async Non-Blocking**: Long-running or IO-bound operations must offload to task pools or stream progress via WebSockets rather than blocking Axum worker threads.
