# Isolate Interfaces & Errors

## Scope
`acp` currently borrows the daemon's `AppError` and core traits (`WorkspaceManager`, `PermissionManager`).

## Acceptance Criteria
- Create a dedicated `AcpError` enum inside the ACP module.
- Implement `From<AcpError> for AppError` in the daemon.
- Migrate `WorkspaceManager` and `PermissionManager` traits into the ACP module boundary (or a shared `acp-interfaces` library).
- Remove `use crate::interfaces::*` imports from `src/acp` where appropriate.
