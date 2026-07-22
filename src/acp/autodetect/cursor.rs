//! Cursor Agent harness discovery.
//!
//! Models come from `agent --list-models` when the CLI is authenticated.
//! No hardcoded catalog — empty list if the probe fails.

use std::path::Path;
use std::pin::Pin;

use crate::config::AgentModel;

use super::common::{run_bounded, strip_ansi};
use super::{AutodetectOptions, Harness};

pub(super) struct Cursor;

impl Harness for Cursor {
    fn id(&self) -> &'static str {
        "cursor"
    }

    fn name(&self) -> &'static str {
        "Cursor Agent"
    }

    fn commands(&self) -> &'static [&'static str] {
        &["agent", "cursor-agent"]
    }

    fn args(&self) -> &'static [&'static str] {
        &["acp"]
    }

    fn search_paths(&self) -> &'static [&'static str] {
        &["%LOCALAPPDATA%\\cursor-agent", "~/.local/bin"]
    }

    fn detect_models(
        &self,
        command: &Path,
        options: AutodetectOptions,
    ) -> Pin<Box<dyn std::future::Future<Output = (Vec<AgentModel>, String)> + Send + '_>> {
        let command = command.to_path_buf();
        Box::pin(async move {
            let models = models_from_cli(&command, options.probe_timeout).await;
            (models, String::new())
        })
    }
}

async fn models_from_cli(command: &Path, duration: std::time::Duration) -> Vec<AgentModel> {
    let Ok((stdout, _)) = run_bounded(command, &["--list-models"], duration).await else {
        return Vec::new();
    };
    parse_list_models(&stdout)
}

/// Parse `agent --list-models` / `agent models` text output.
fn parse_list_models(output: &[u8]) -> Vec<AgentModel> {
    let text = String::from_utf8_lossy(output);
    if text.to_ascii_lowercase().contains("no models available") {
        return Vec::new();
    }
    text.lines()
        .filter_map(|line| {
            let line = strip_ansi(line).trim().to_owned();
            let (id, name) = line.split_once(" - ")?;
            let id = id.trim();
            if id.is_empty() {
                return None;
            }
            let name = strip_status_suffix(name.trim());
            Some(AgentModel::new(id.into(), name.into()))
        })
        .collect()
}

/// Remove trailing Cursor status markers from a model display name.
fn strip_status_suffix(name: &str) -> &str {
    let trimmed = name.trim();
    // Match " (…)" only when the paren content looks like a status tag
    // (current / default / both), not model names that legitimately contain
    // parentheses like "Claude 4.5 Opus (Thinking)".
    if let Some(open) = trimmed.rfind('(') {
        if open > 0 && trimmed.ends_with(')') {
            let tag = trimmed[open + 1..trimmed.len() - 1]
                .trim()
                .to_ascii_lowercase();
            if tag == "current"
                || tag == "default"
                || tag == "current, default"
                || tag == "default, current"
            {
                return trimmed[..open].trim_end();
            }
        }
    }
    trimmed
}

#[cfg(test)]
mod tests {
    use super::{parse_list_models, strip_status_suffix};
    use crate::config::AgentModel;

    fn assert_multi_model_shape(models: &[AgentModel]) {
        assert!(models.len() >= 2, "expected multiple models");
        let mut seen = std::collections::HashSet::new();
        for model in models {
            assert!(!model.id.is_empty());
            assert!(!model.name.is_empty());
            assert!(seen.insert(model.id.as_str()), "duplicate {}", model.id);
        }
    }

    #[test]
    fn list_models_parser_yields_multiple_clean_entries() {
        let models = parse_list_models(
            b"Loading models\nAvailable models\n\n\
              auto - Auto (current, default)\n\
              model-one - Model One\n\
              model-two - Model Two (Thinking)\n\
              model-three - Model Three (current)\n\
              No separator\n",
        );
        assert_multi_model_shape(&models);
        assert_eq!(models.len(), 4);
        // Status tags stripped; real parentheticals kept.
        assert_eq!(models[0].name, "Auto");
        assert!(models[2].name.contains("Thinking"));
        assert_eq!(models[3].name, "Model Three");
        assert!(!models.iter().any(|m| m.name.contains("current")));
    }

    #[test]
    fn list_models_parser_handles_empty_and_unavailable() {
        assert!(parse_list_models(b"").is_empty());
        assert!(parse_list_models(b"No models available\n").is_empty());
    }

    #[test]
    fn status_suffix_helper() {
        assert_eq!(strip_status_suffix("Auto (current, default)"), "Auto");
        assert_eq!(strip_status_suffix("X (Thinking)"), "X (Thinking)");
    }
}
