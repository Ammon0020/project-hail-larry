//! Codex CLI (ACP adapter) harness discovery.

use std::path::Path;
use std::pin::Pin;

use serde::Deserialize;

use crate::config::AgentModel;

use super::{AutodetectOptions, Harness};

pub(super) struct Codex;

impl Harness for Codex {
    fn id(&self) -> &'static str {
        "codex"
    }

    fn name(&self) -> &'static str {
        "Codex CLI"
    }

    fn commands(&self) -> &'static [&'static str] {
        // Never add the bare interactive `codex` TUI. Only its ACP adapter
        // speaks the stdio protocol and is safe to start with pipes.
        &["codex-acp"]
    }

    fn args(&self) -> &'static [&'static str] {
        &[]
    }

    fn detect_models(
        &self,
        _command: &Path,
        _options: AutodetectOptions,
    ) -> Pin<Box<dyn std::future::Future<Output = (Vec<AgentModel>, String)> + Send + '_>> {
        Box::pin(async { (models_from_cache(), String::new()) })
    }
}

fn models_from_cache() -> Vec<AgentModel> {
    #[derive(Deserialize)]
    struct Cache {
        #[serde(default)]
        models: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        slug: String,
        #[serde(default)]
        display_name: String,
    }
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(home.join(".codex/models_cache.json")) else {
        return Vec::new();
    };
    serde_json::from_str::<Cache>(&contents)
        .map(|cache| {
            cache
                .models
                .into_iter()
                .filter(|model| !model.slug.is_empty())
                .map(|model| {
                    let name = if model.display_name.is_empty() {
                        model.slug.clone()
                    } else {
                        model.display_name
                    };
                    AgentModel::new(model.slug, name)
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::models_from_cache;

    #[test]
    fn cache_parse_is_empty_without_file_or_returns_shape() {
        // Without a fixture file this is environment-dependent. Only assert
        // that the function is safe (no panic) and that any returned models
        // have non-empty ids. Specific model names are not pinned.
        let models = models_from_cache();
        for model in &models {
            assert!(!model.id.is_empty());
            assert!(!model.name.is_empty());
        }
        if models.len() >= 2 {
            let ids: std::collections::HashSet<_> = models.iter().map(|m| m.id.as_str()).collect();
            assert_eq!(ids.len(), models.len());
        }
    }
}
