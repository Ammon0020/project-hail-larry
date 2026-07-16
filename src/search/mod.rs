//! Workspace content search (Go `internal/search/`).
//!
//! File-content search over registered workspaces. Search DTOs
//! ([`crate::interfaces::SearchOptions`], [`crate::interfaces::SearchResult`])
//! live in [`crate::interfaces`] so this module does not become a dependency
//! sink for the trait layer (epic.md Phase 1 / S-INTERFACES). Implementation
//! lands in S-SEARCH (Phase 2). S-ARCH scope: module placeholder only.

// Re-export search DTOs for ergonomic `search::SearchOptions` access once the
// implementation lands. Consumers of the trait layer should prefer
// `interfaces::{SearchOptions, SearchResult}`.
pub use crate::interfaces::{SearchOptions, SearchResult};
