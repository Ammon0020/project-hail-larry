# Abstract Shell Execution

## Scope
`src/acp/core/handlers/terminal.rs` currently hardcodes the usage of `crate::shell::Executor` and internal environment filters. A consuming app (like Tauri) may want to handle shell execution natively or disable it completely.

## Acceptance Criteria
- Define an async `ShellExecutor` trait in the ACP module that abstracts `run_command`.
- Update `HandlerDeps` to accept `Arc<dyn ShellExecutor>`.
- Keep ACP terminal protocol state logic inside ACP.
- Implement `ShellExecutor` for the daemon's `crate::shell::Executor`.

## Notes
Subagents can be deployed to refactor `terminal.rs` and verify the executor boundaries.
