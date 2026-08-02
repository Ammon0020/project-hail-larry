# src/api/

## Responsibility

Daemon HTTP/REST and WebSocket API route handlers serving frontend client requests.

## Module Map

- **`mod.rs`** — Axum web router setup, API endpoint registrations, authentication middleware, and error handling.
- **`agents.rs`** — Agent registration, removal, and executable autodetection endpoints.
- **`auth.rs`** — Request authentication middleware, peer extraction, and preview credential checks.
- **`devices.rs`** — Paired-device listing, revocation, and device pending-action endpoints.
- **`embed.rs`** — Static web asset embedding and SPA fallback handler.
- **`events.rs`** — Event history queries, session filtering, and pagination limits.
- **`files.rs`** — Workspace file tree, reads/writes, mutations, raw serving, and search endpoints.
- **`git.rs`** — Git workspace operations, diff generation, branch control, and staging endpoints.
- **`mcp.rs`** — Model Context Protocol (MCP) server status and configuration endpoints.
- **`pair.rs`** — Pairing initiation/verification endpoints and per-peer rate limiting.
- **`permissions.rs`** — Pending permission listing and response endpoints.
- **`preview.rs`** — Preview-session tickets, authorization, cookies, and preview file serving.
- **`profiles.rs`** — Agent profile management endpoints (CRUD for agent configurations and options).
- **`providers.rs`** — Available LLM/ACP provider listing and status endpoints.
- **`sessions.rs`** — Session lifecycle, prompts, cancellation, and profile validation endpoints.
- **`session_extra.rs`** — Extended session operations, file uploads, and auxiliary context attachments.
- **`settings.rs`** — User preference and daemon configuration update endpoints.
- **`test_support.rs`** — Shared test-state construction and router dispatch helpers for API tests.
- **`workspaces.rs`** — Workspace registration/removal, trust, and pending-registration endpoints.

## Rules & Patterns

- **Path Security**: All file and directory endpoints must validate paths against registered workspace boundaries; reject symlinks and path traversal attempts.
- **Error Serialization**: Use structured error responses (`ApiError`) with standard HTTP status codes; do not leak internal stack traces to unauthenticated clients.
- **Async Non-Blocking**: Long-running or IO-bound operations must offload to task pools or stream progress via WebSockets rather than blocking Axum worker threads.
