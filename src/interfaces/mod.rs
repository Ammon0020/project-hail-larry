//! Shared traits and typed errors (Go `internal/interfaces/`).
//!
//! Cross-module `trait` definitions (`EventStore`, `WorkspaceManager`,
//! `PermissionManager`, etc.) and the `AppError` enum. Search DTOs live here
//! too so `interfaces` does not depend on `search` (epic.md Phase 1 note).
//! Implementation lands in S-INTERFACES (Phase 1). S-ARCH scope: module
//! placeholder only.
