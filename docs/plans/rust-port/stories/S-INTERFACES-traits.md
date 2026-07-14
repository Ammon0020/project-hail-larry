# Story S-INTERFACES: Shared Trait Definitions

> **Phase:** 1 | **Depends on:** S-PATHUTIL | **Go source:** `internal/interfaces/` (434 lines)

## Summary

Port the shared interface contracts (`EventStore`, `WorkspaceManager`,
`ACPClient`, `PermissionManager`, `FileSync`, `ACPCallbacks`) to Rust
traits. Also port the event type enum, `Event`/`Attachment`/`SessionInfo`
structs, and all shared data types.

## Go Source

`internal/interfaces/interfaces.go` — defines all cross-package contracts.
This is the architectural backbone: every package implements against these
interfaces.

## Rust Implementation

- Keep shared DTOs in a dependency-free `types` module. Move `search::Options`
  and `search::Result` equivalents there so the service trait layer does not
  depend on the search implementation.
- Use traits only at real replacement/test boundaries. `AppState` may hold
  concrete services where no alternate implementation is needed; do not mirror
  every Go interface with `Arc<dyn Trait>` by default.
- Rust internal events are a typed enum plus common metadata. A dedicated wire
  adapter must serialize the current flat Go JSON shapes exactly; do not make a
  serde enum representation the public contract without S-CONTRACT fixtures.
- Define layer-specific `thiserror` error enums and a single API-error mapper;
  stale revisions, missing resources, unsupported capabilities, and validation
  failures must not be distinguished by error strings.
- `Attachment.URI` has `json:"-"` → `#[serde(skip)]`.
- All service dependencies are constructor arguments. `EventPublisher` is a
  narrow dependency for durable app-event publication, not a general callback
  or command bus.

### Key types to port

- `EventType` (27 variants) + constants
- `Event`, `Attachment`
- `FileNode`, `WorkspaceInfo`
- `AgentInfo`, `AgentModel`, `SessionInfo`, `Session`
- `ProviderInfo`, `ProviderCurrentConfig`
- `PermissionRequest`, `PermissionResponse`, `PermissionOption`
- Traits: `EventStore`, `WorkspaceManager`, `ACPClient`, `PermissionManager`,
  `FileSync`, `ACPCallbacks`

## Acceptance Criteria

- [ ] Shared search DTOs have no dependency on the search implementation
- [ ] Traits exist only at documented replacement/test boundaries
- [ ] Typed internal events and errors have a stable wire adapter
- [ ] All shared structs serialize to identical JSON as Go through S-CONTRACT
- [ ] All required service dependencies are constructor arguments
- [ ] `cargo check` passes
- [ ] `cargo clippy -- -D warnings` passes
