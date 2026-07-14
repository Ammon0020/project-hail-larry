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

- Module: `interfaces` (or distribute types across modules with re-exports)
- Go `interface` → Rust `trait` (use `dyn Trait` for runtime dispatch in
  `AppState`, or generics if static dispatch is preferred)
- `EventType` string enum → `#[derive(Serialize)] enum EventType`
- `Event` struct → `#[derive(Serialize, Deserialize, Clone)] struct Event`
  with `#[serde(rename_all = "camelCase")]` to match Go's JSON tags
- `Attachment.URI` has `json:"-"` → `#[serde(skip)]`
- `ACPCallbacks.OnEvent` → `trait ACPCallbacks { fn on_event(&self, event: Event); }`

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

- [ ] All traits defined with async methods (`async fn`)
- [ ] All shared structs serialize to identical JSON as Go (verify with
      side-by-side comparison)
- [ ] `cargo check` passes
- [ ] `cargo clippy` clean
