//! Safe discovery of the small, audited set of ACP harnesses.
//!
//! No user-configured command string is ever executed here: every executable
//! and every argument is declared by a harness module, then passed directly to
//! `tokio::process::Command`.
//!
//! Each harness lives in its own file under this module so model probes can be
//! maintained independently without breaking other agents.

mod claude_code;
mod codex;
mod common;
mod cursor;
mod devin;
mod vibe;

use std::path::Path;
use std::time::Duration;

use crate::config::{AgentInfo, AgentModel};

use self::common::{find_first_command, probe_providers};

/// Registry of known harnesses. Order is stable for config/UI presentation.
const HARNESSES: &[&dyn Harness] = &[
    &claude_code::ClaudeCode,
    &codex::Codex,
    &cursor::Cursor,
    &devin::Devin,
    &vibe::Vibe,
];

/// Trait implemented by each known ACP harness detector.
///
/// Implementations must never hardcode long-lived model catalogs as the source
/// of truth; they may only probe the agent/CLI/config and return what is found.
pub(crate) trait Harness: Send + Sync {
    fn id(&self) -> &'static str;
    fn name(&self) -> &'static str;
    fn commands(&self) -> &'static [&'static str];
    fn args(&self) -> &'static [&'static str];
    fn search_paths(&self) -> &'static [&'static str] {
        &[]
    }

    /// Discover models for an installed command. Empty means unavailable.
    fn detect_models(
        &self,
        command: &Path,
        options: AutodetectOptions,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = (Vec<AgentModel>, String)> + Send + '_>>;
}

/// Whether the unstable ACP `providers/list` capability may be queried.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderProbe {
    /// Skip the unstable live query.
    #[default]
    Disabled,
    /// Send bounded ACP initialize and providers/list requests over stdio.
    Enabled,
}

/// Discovery policy. Defaults are safe for daemon startup.
#[derive(Debug, Clone, Copy)]
pub struct AutodetectOptions {
    pub provider_probe: ProviderProbe,
    pub probe_timeout: Duration,
    /// Bound for interactive browser-auth model probes (Devin). Separate from
    /// `probe_timeout` so a first-time login does not block daemon startup for
    /// the full browser window unless the caller opts in with a longer value.
    pub auth_probe_timeout: Duration,
}

impl Default for AutodetectOptions {
    fn default() -> Self {
        Self {
            provider_probe: ProviderProbe::Disabled,
            probe_timeout: Duration::from_secs(8),
            // First-time browser PKCE; kill the child if the user does not finish.
            auth_probe_timeout: Duration::from_secs(90),
        }
    }
}

/// Returns valid bare commands for a known agent, or `None` for custom agents.
#[must_use]
pub fn valid_commands_for_agent(id: &str) -> Option<Vec<&'static str>> {
    HARNESSES
        .iter()
        .find(|h| h.id() == id)
        .map(|h| h.commands().to_vec())
}

/// Detects installed known ACP agents in stable registry order.
pub async fn autodetect() -> Vec<AgentInfo> {
    autodetect_with(AutodetectOptions::default()).await
}

/// Detects installed known ACP agents using an explicit live-probe policy.
pub async fn autodetect_with(options: AutodetectOptions) -> Vec<AgentInfo> {
    let mut agents = Vec::new();
    for harness in HARNESSES {
        let Some(command) = find_first_command(harness.commands(), harness.search_paths()) else {
            continue;
        };
        let (models, warning) = detect_models_for(*harness, &command, options).await;
        agents.push(AgentInfo {
            id: harness.id().into(),
            name: harness.name().into(),
            command: command.to_string_lossy().into_owned(),
            args: harness.args().iter().map(|arg| (*arg).into()).collect(),
            models,
            warning,
        });
    }
    agents
}

async fn detect_models_for(
    harness: &dyn Harness,
    command: &Path,
    options: AutodetectOptions,
) -> (Vec<AgentModel>, String) {
    if options.provider_probe == ProviderProbe::Enabled {
        if let Ok(models) = probe_providers(command, harness.args(), options.probe_timeout).await {
            if !models.is_empty() {
                return (models, String::new());
            }
        }
    }
    harness.detect_models(command, options).await
}

/// Merge configured agents with a fresh autodetect pass.
///
/// Existing entries keep user-set name/command; empty fields are filled from
/// detection. Model lists from a successful probe replace stale ones. New IDs
/// are appended. Returns the merged list and whether anything changed.
#[must_use]
pub fn merge_autodetected_agents(
    configured: &[AgentInfo],
    detected: Vec<AgentInfo>,
) -> (Vec<AgentInfo>, bool) {
    let mut merged = configured.to_vec();
    let mut changed = false;
    for detected_agent in detected {
        if let Some(slot) = merged
            .iter_mut()
            .find(|agent| agent.id == detected_agent.id)
        {
            if slot.name.is_empty() {
                slot.name = detected_agent.name;
                changed = true;
            }
            if slot.command.is_empty() {
                slot.command = detected_agent.command;
                changed = true;
            }
            if slot.args.is_empty() && !detected_agent.args.is_empty() {
                slot.args = detected_agent.args;
                changed = true;
            }
            // Replace when probe returned models. Keep a previously-loaded
            // real list when the probe returns empty (e.g. temporarily
            // unauthed), but clear entries that were only hardcoded fallbacks.
            if !detected_agent.models.is_empty() {
                if slot.models != detected_agent.models {
                    slot.models = detected_agent.models;
                    changed = true;
                }
            } else if slot.warning == "Using fallback model list" && !slot.models.is_empty() {
                slot.models.clear();
                changed = true;
            }
            if slot.warning != detected_agent.warning {
                slot.warning = detected_agent.warning;
                changed = true;
            }
        } else {
            merged.push(detected_agent);
            changed = true;
        }
    }
    (merged, changed)
}

/// Drop persisted known-agent entries whose command is no longer a valid launch
/// command for that agent id. Custom agents are kept.
#[must_use]
pub fn prune_stale_known_agents(agents: Vec<AgentInfo>) -> (Vec<AgentInfo>, bool) {
    let mut pruned = Vec::with_capacity(agents.len());
    let mut removed = false;
    for agent in agents {
        match valid_commands_for_agent(&agent.id) {
            None => pruned.push(agent),
            Some(valid) if command_matches_spec(&agent.command, &valid) => pruned.push(agent),
            Some(valid) => {
                tracing::warn!(
                    agent_id = %agent.id,
                    command = %agent.command,
                    ?valid,
                    "removing stale agent entry: command is not a valid launch command"
                );
                removed = true;
            }
        }
    }
    (pruned, removed)
}

/// True when `cmd` is a bare valid name or a path whose base matches one.
fn command_matches_spec(cmd: &str, valid_commands: &[&str]) -> bool {
    if cmd.is_empty() {
        return false;
    }
    let base = Path::new(cmd)
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or(cmd);
    for valid in valid_commands {
        if cmd == *valid || base == *valid {
            return true;
        }
        #[cfg(windows)]
        {
            let base_lower = base.to_ascii_lowercase();
            let valid_lower = valid.to_ascii_lowercase();
            if base_lower == valid_lower
                || base_lower.trim_end_matches(".exe") == valid_lower
                || base_lower.trim_end_matches(".cmd") == valid_lower
            {
                return true;
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::{
        command_matches_spec, merge_autodetected_agents, prune_stale_known_agents,
        valid_commands_for_agent, ProviderProbe, HARNESSES,
    };
    use crate::config::{AgentInfo, AgentModel};

    #[test]
    fn harness_registry_is_non_empty_and_unique() {
        assert!(!HARNESSES.is_empty());
        let mut ids: Vec<&str> = HARNESSES.iter().map(|h| h.id()).collect();
        let before = ids.len();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), before, "duplicate harness ids");
    }

    #[test]
    fn codex_never_allows_the_bare_tui() {
        let commands = valid_commands_for_agent("codex").expect("known codex agent");
        assert_eq!(commands, ["codex-acp"]);
        assert!(!commands.contains(&"codex"));
        assert!(valid_commands_for_agent("custom").is_none());
    }

    #[test]
    fn merge_adds_new_and_fills_empty_fields() {
        let configured = vec![AgentInfo {
            id: "codex".into(),
            name: String::new(),
            command: String::new(),
            args: Vec::new(),
            models: Vec::new(),
            warning: String::new(),
        }];
        let detected = vec![
            AgentInfo {
                id: "codex".into(),
                name: "Codex CLI".into(),
                command: "/usr/bin/codex-acp".into(),
                args: Vec::new(),
                models: vec![AgentModel {
                    id: "gpt".into(),
                    name: "GPT".into(),
                }],
                warning: String::new(),
            },
            AgentInfo {
                id: "cursor".into(),
                name: "Cursor Agent".into(),
                command: "/usr/bin/agent".into(),
                args: vec!["acp".into()],
                models: Vec::new(),
                warning: String::new(),
            },
        ];
        let (merged, changed) = merge_autodetected_agents(&configured, detected);
        assert!(changed);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].name, "Codex CLI");
        assert_eq!(merged[0].command, "/usr/bin/codex-acp");
        assert_eq!(merged[0].models.len(), 1);
        assert_eq!(merged[1].id, "cursor");
    }

    #[test]
    fn merge_replaces_stale_model_list_when_probe_returns_models() {
        let configured = vec![AgentInfo {
            id: "cursor".into(),
            name: "Cursor Agent".into(),
            command: "/usr/bin/agent".into(),
            args: vec!["acp".into()],
            models: vec![
                AgentModel {
                    id: "stale-fallback".into(),
                    name: "Stale".into(),
                },
                AgentModel {
                    id: "auto".into(),
                    name: "Auto  (current, default)".into(),
                },
            ],
            warning: "Using fallback model list".into(),
        }];
        let detected = vec![AgentInfo {
            id: "cursor".into(),
            name: "Cursor Agent".into(),
            command: "/usr/bin/agent".into(),
            args: vec!["acp".into()],
            models: vec![
                AgentModel {
                    id: "auto".into(),
                    name: "Auto".into(),
                },
                AgentModel {
                    id: "m2".into(),
                    name: "Model Two".into(),
                },
            ],
            warning: String::new(),
        }];
        let (merged, changed) = merge_autodetected_agents(&configured, detected);
        assert!(changed);
        assert_eq!(merged[0].models.len(), 2);
        assert_eq!(merged[0].models[0].name, "Auto");
        assert!(merged[0].warning.is_empty());
    }

    #[test]
    fn merge_keeps_real_models_when_probe_returns_empty() {
        let configured = vec![AgentInfo {
            id: "devin".into(),
            name: "Devin".into(),
            command: "devin".into(),
            args: vec!["acp".into()],
            models: vec![AgentModel {
                id: "previously-loaded".into(),
                name: "Previously Loaded".into(),
            }],
            warning: String::new(),
        }];
        let detected = vec![AgentInfo {
            id: "devin".into(),
            name: "Devin".into(),
            command: "devin".into(),
            args: vec!["acp".into()],
            models: Vec::new(),
            warning: String::new(),
        }];
        let (merged, changed) = merge_autodetected_agents(&configured, detected);
        assert!(!changed);
        assert_eq!(merged[0].models.len(), 1);
    }

    #[test]
    fn merge_clears_stale_fallback_models_when_probe_returns_empty() {
        let configured = vec![AgentInfo {
            id: "devin".into(),
            name: "Devin".into(),
            command: "devin".into(),
            args: vec!["acp".into()],
            models: vec![AgentModel {
                id: "stale".into(),
                name: "Stale".into(),
            }],
            warning: "Using fallback model list".into(),
        }];
        let detected = vec![AgentInfo {
            id: "devin".into(),
            name: "Devin".into(),
            command: "devin".into(),
            args: vec!["acp".into()],
            models: Vec::new(),
            warning: String::new(),
        }];
        let (merged, changed) = merge_autodetected_agents(&configured, detected);
        assert!(changed);
        assert!(merged[0].models.is_empty());
        assert!(merged[0].warning.is_empty());
    }

    #[test]
    fn prune_drops_stale_codex_tui_keeps_custom() {
        let agents = vec![
            AgentInfo {
                id: "codex".into(),
                name: "Codex".into(),
                command: "codex".into(),
                args: Vec::new(),
                models: Vec::new(),
                warning: String::new(),
            },
            AgentInfo {
                id: "custom".into(),
                name: "Custom".into(),
                command: "/opt/my-agent".into(),
                args: Vec::new(),
                models: Vec::new(),
                warning: String::new(),
            },
        ];
        let (pruned, removed) = prune_stale_known_agents(agents);
        assert!(removed);
        assert_eq!(pruned.len(), 1);
        assert_eq!(pruned[0].id, "custom");
        assert!(command_matches_spec(
            "/home/u/.nvm/bin/codex-acp",
            &["codex-acp"]
        ));
    }

    #[test]
    fn provider_probe_is_explicitly_opt_in() {
        assert_eq!(ProviderProbe::default(), ProviderProbe::Disabled);
    }

    /// Generic shape check: a successful multi-model parse must yield unique
    /// non-empty ids. Specific model names are not asserted (they churn).
    fn assert_multi_model_shape(models: &[AgentModel]) {
        assert!(
            models.len() >= 2,
            "expected multiple models, got {}",
            models.len()
        );
        let mut seen = std::collections::HashSet::new();
        for model in models {
            assert!(!model.id.is_empty(), "empty model id");
            assert!(!model.name.is_empty(), "empty model name for {}", model.id);
            assert!(
                seen.insert(model.id.as_str()),
                "duplicate model id {}",
                model.id
            );
        }
    }

    #[test]
    fn generic_multi_model_shape_helper() {
        assert_multi_model_shape(&[
            AgentModel {
                id: "a".into(),
                name: "A".into(),
            },
            AgentModel {
                id: "b".into(),
                name: "B".into(),
            },
        ]);
    }
}
