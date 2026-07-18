//! Shared traits, wire types, and typed errors (Go `internal/interfaces/`).
//!
//! Architectural backbone for the Rust port: every service package implements
//! against these contracts. Layout:
//!
//! - [`types`]  — dependency-free shared DTOs (events, workspace, session,
//!   permissions, search options/results, pairing/device wire shapes)
//! - [`traits`] — service traits at real replacement/test boundaries
//! - [`error`]  — layer-specific `thiserror` enums + API error mapper
//! - [`wire`]   — typed internal events ↔ flat Go JSON adapter
//!
//! Design rules (epic.md + S-INTERFACES story):
//! - Traits only at documented replacement/test boundaries; `AppState` may hold
//!   concrete services where no alternate implementation is needed.
//! - Search DTOs live here so the trait layer never depends on `search`.
//! - Agent DTOs (`AgentInfo`, `AgentModel`) live in [`crate::config`] and are
//!   re-exported from [`types`].
//! - All service dependencies are constructor arguments. [`EventPublisher`] is
//!   narrow: durable app-event publication only, not a general callback bus.
//! - Public wire contract is the flat Go JSON shape; do not expose a serde enum
//!   as the public contract without S-CONTRACT fixtures.
//!
//! See `docs/plans/rust-port/complete-S-INTERFACES-traits-med.md`.

pub mod error;
pub mod traits;
pub mod types;
pub mod wire;

#[cfg(test)]
mod tests;

// Re-export the public surface so callers can `use local_agent::interfaces::*`.
pub use error::{map_api_error, ApiError, ApiErrorBody, ApiStatusCode, AppError};
pub use traits::{
    ACPCallbacks, ACPClient, EventPublisher, EventStore, FileSync, PermissionManager,
    ReadFileResult, WorkspaceManager,
};
pub use types::{
    go_zero_time, AgentInfo, AgentModel, Attachment, DeviceCredential, DeviceInfo, Event,
    EventMeta, EventPayload, EventType, FileNode, PairingSession, PendingActionInfo,
    PermissionDecision, PermissionOption, PermissionOptionInfo, PermissionRequest,
    PermissionResponse, ProviderCurrentConfig, ProviderInfo, SearchOptions, SearchResult, Session,
    SessionInfo, TypedEvent, WorkspaceInfo, FILE_NODE_TYPE_FILE, FILE_NODE_TYPE_FOLDER,
    PENDING_ACTION_TYPE_REVOCATION, PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION,
};
pub use wire::{event_to_json_pretty, typed_event, typed_event_to_wire, wire_to_typed_event};
