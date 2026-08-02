# src/

## Responsibility

Rust daemon library and binaries.
- Library entry: `src/lib.rs`
- Binary entry: `src/main.rs` (`local_agent`)
- Mock agent: `src/bin/mockagent.rs`

## Module Map

```text
src/
├── acp/          ACP boundary: actor, context, providers (See acp/AGENTS.md)
├── api/          REST/WS handlers (See api/AGENTS.md)
├── app/          daemon, listeners, TLS (See app/AGENTS.md)
├── cli/          commands and service installers
├── config/       config model/store
├── events/       SQLite WAL event bus
├── files/        client file surface
├── fswatch/      filesystem watcher
├── git/          Git operations
├── interfaces/   wire types, DTOs, traits
├── mcp/          MCP integration
├── migrate/      config migration/validation
├── pairing/      QR/mnemonic credentials
├── permissions/  permission manager/sink
├── search/       ripgrep/native search
├── shell/        scoped shell execution
├── sync/         WebSocket sync/merge
├── uploads/      session upload store
└── workspace/    registration/path containment
```

## Rules & Patterns

- `acp/` is the only agent integration boundary; no per-agent integrations elsewhere.
- Filesystem, shell, permissions, and sessions live in the daemon; the web client never calls an agent directly.
- Contain and validate all workspace paths; reject symlinks.
- Use `thiserror` for domain errors and `anyhow` for ad-hoc wrapping.
- Prefer small modules; avoid megafiles.
