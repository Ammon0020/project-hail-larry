//! Agent Client Protocol client (Go `internal/acp/`).
//!
//! This module currently provides the configured-harness registry and safe
//! known-agent discovery. Session lifecycle and callback handling land in the
//! remaining S-ACP stories.

mod agent_registry;
mod autodetect;

pub use agent_registry::AgentRegistry;
pub use autodetect::{
    autodetect, autodetect_with, valid_commands_for_agent, AutodetectOptions, ProviderProbe,
};
