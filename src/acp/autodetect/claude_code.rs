//! Claude Code harness discovery.

use std::path::Path;
use std::pin::Pin;

use crate::config::AgentModel;

use super::{AutodetectOptions, Harness};

pub(super) struct ClaudeCode;

impl Harness for ClaudeCode {
    fn id(&self) -> &'static str {
        "claude-code"
    }

    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn commands(&self) -> &'static [&'static str] {
        &["claude"]
    }

    fn args(&self) -> &'static [&'static str] {
        &[]
    }

    fn detect_models(
        &self,
        _command: &Path,
        _options: AutodetectOptions,
    ) -> Pin<Box<dyn std::future::Future<Output = (Vec<AgentModel>, String)> + Send + '_>> {
        // No stable non-interactive model list yet. Session config probe can
        // be added here later without touching other harnesses.
        Box::pin(async { (Vec::new(), String::new()) })
    }
}
