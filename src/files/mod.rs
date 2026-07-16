//! Revision tracking and three-way merge (Go `internal/files/`).
//!
//! 48-bit content hashing, per-file locking, and a 256-entry LRU base-content
//! cache for merge. Implementation lands in S-FILES (Phase 2). S-ARCH scope:
//! module placeholder only.
