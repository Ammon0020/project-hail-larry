//! Agent Client Protocol client (Go `internal/acp/`).
//!
//! Wraps the official `agent_client_protocol` Rust SDK. Owns session
//! lifecycle, streaming, tool callbacks (file read/write, shell), permission
//! relay, provider management, and MCP relay. Implementation lands in the
//! S-ACP-* story family (Phase 3). S-ARCH scope: module placeholder only.
