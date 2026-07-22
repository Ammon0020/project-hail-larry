//! Mistral Vibe harness discovery.

use std::path::Path;
use std::pin::Pin;

use serde::Deserialize;

use crate::config::AgentModel;

use super::{AutodetectOptions, Harness};

pub(super) struct Vibe;

impl Harness for Vibe {
    fn id(&self) -> &'static str {
        "mistral-vibe"
    }

    fn name(&self) -> &'static str {
        "Mistral Vibe"
    }

    fn commands(&self) -> &'static [&'static str] {
        &["vibe-acp", "vibe"]
    }

    fn args(&self) -> &'static [&'static str] {
        &[]
    }

    fn detect_models(
        &self,
        _command: &Path,
        _options: AutodetectOptions,
    ) -> Pin<Box<dyn std::future::Future<Output = (Vec<AgentModel>, String)> + Send + '_>> {
        Box::pin(async { (models_from_config(), String::new()) })
    }
}

fn models_from_config() -> Vec<AgentModel> {
    #[derive(Deserialize)]
    struct Config {
        #[serde(default)]
        models: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        name: String,
        #[serde(default)]
        alias: String,
    }
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    let Ok(contents) = std::fs::read_to_string(home.join(".vibe/config.toml")) else {
        return Vec::new();
    };
    toml::from_str::<Config>(&contents)
        .map(|config| {
            config
                .models
                .into_iter()
                .filter(|model| !model.name.is_empty())
                .map(|model| AgentModel {
                    id: if model.alias.is_empty() {
                        model.name.clone()
                    } else {
                        model.alias
                    },
                    name: model.name,
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::models_from_config;

    #[test]
    fn config_parse_is_safe_and_shape_checked() {
        let models = models_from_config();
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
