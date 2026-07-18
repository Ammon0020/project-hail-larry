//! Agent Client Protocol client (Go `internal/acp/`).
//!
//! This module currently provides the configured-harness registry and safe
//! known-agent discovery. Session lifecycle and callback handling land in the
//! remaining S-ACP stories.

mod agent_registry;
mod autodetect;
mod context;
mod conversation;
mod core;
mod profile;
mod providers;
mod store;
mod stream;

pub use agent_registry::AgentRegistry;
pub use autodetect::{
    autodetect, autodetect_with, merge_autodetected_agents, prune_stale_known_agents,
    valid_commands_for_agent, AutodetectOptions, ProviderProbe,
};
pub use context::{EditorSelection, OpenFilesTracker};
pub use conversation::export_conversation;
pub use core::{Client, ClientDeps, SessionState, STDERR_TAIL_BYTES};
pub use providers::SessionCaps;
pub use store::ConversationStore;
