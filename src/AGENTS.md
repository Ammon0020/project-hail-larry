# src/

## Responsibility

Rust daemon library and binaries.
- Library entry: `src/lib.rs`
- Binary entry: `src/main.rs` (`local_agent`)
- Mock agent: `src/bin/mockagent.rs`

## Module Map

- **`acp/`** — ACP integration boundary: actor, autodetect, profiles, providers, store, stream. (See `src/acp/AGENTS.md`)
- **`api/`** — REST handlers: embed, git, mcp, profiles, providers, settings, session extras. (See `src/api/AGENTS.md`)
- **`app/`** — Daemon: listener, TLS, rate limiting, process mgmt, logging. (See `src/app/AGENTS.md`)
- `cli/` — CLI commands + per-OS service installers.
- `config/` — Config model and `~/.local-agent` file store.
- `events/` — Append-only SQLite WAL event store, publisher, replay.
- `files/` — Client-side file read/write surface.
- `shell/` — Scoped shell execution.
- `permissions/` — Permission manager and sink for file/shell prompts.
- `workspace/` — Workspace registration and path containment.
- `sync/` — WebSocket sync and three-way merge.
- `pairing/` — QR/mnemonic pairing and expiring credentials.
- `interfaces/` — ACP wire types, traits, DTOs shared with the web client.
- `mcp/` — MCP integration.
- `migrate/` — Config migration/validation.
- `fsutil/`, `pathutil/`, `procutil/` — Cross-platform fs/path/process helpers.
- `fswatch/` — Filesystem watcher + debouncer.
- `search/` — Content search (ripgrep + native fallback).
- `uploads/` — Per-session upload store.
- `git/` — Git operations and porcelain API.

## Rules & Patterns

- `acp/` is the only agent integration boundary; no per-agent integrations elsewhere.
- Filesystem, shell, permissions, and sessions live in the daemon; the web client never calls an agent directly.
- Contain and validate all workspace paths; reject symlinks.
- Use `thiserror` for domain errors and `anyhow` for ad-hoc wrapping.
- Prefer small modules; avoid megafiles.
