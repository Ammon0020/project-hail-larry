//! Thread-safe, deterministic registry for configured ACP harnesses.

use std::collections::BTreeMap;
use std::sync::{PoisonError, RwLock};

use crate::config::AgentInfo;

/// Registered agent descriptors. A `BTreeMap` makes public listing stable.
#[derive(Default)]
pub struct AgentRegistry {
    agents: RwLock<BTreeMap<String, AgentInfo>>,
}

impl AgentRegistry {
    /// Builds a registry from persisted configuration, replacing duplicate IDs
    /// with their final configuration entry just as config upsert does.
    #[must_use]
    pub fn from_agents(agents: impl IntoIterator<Item = AgentInfo>) -> Self {
        let registry = Self::default();
        for agent in agents {
            registry.register(agent);
        }
        registry
    }

    /// Adds or replaces an agent descriptor.
    pub fn register(&self, agent: AgentInfo) {
        let mut agents = self.agents.write().unwrap_or_else(PoisonError::into_inner);
        agents.insert(agent.id.clone(), verify_agent_executable(agent));
    }

    /// Removes an agent. Missing IDs are intentionally a no-op.
    pub fn remove(&self, id: &str) {
        let mut agents = self.agents.write().unwrap_or_else(PoisonError::into_inner);
        agents.remove(id);
    }

    /// Lists public agent data without leaking executable paths or argv.
    #[must_use]
    pub fn list(&self) -> Vec<AgentInfo> {
        let agents = self.agents.read().unwrap_or_else(PoisonError::into_inner);
        agents
            .values()
            .map(|agent| AgentInfo {
                id: agent.id.clone(),
                name: agent.name.clone(),
                command: String::new(),
                args: Vec::new(),
                models: agent.models.clone(),
                warning: agent.warning.clone(),
            })
            .collect()
    }

    /// Returns a complete independent descriptor snapshot.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<AgentInfo> {
        let agents = self.agents.read().unwrap_or_else(PoisonError::into_inner);
        agents.get(id).cloned()
    }

    /// Resolves a configured agent and one of its explicitly offered models.
    ///
    /// # Errors
    /// Returns Go-compatible error text for an unknown agent or model.
    pub fn resolve(&self, agent_id: &str, model_id: &str) -> Result<AgentInfo, String> {
        let Some(agent) = self.get(agent_id) else {
            return Err(format!("agent not found: {agent_id}"));
        };
        if agent.models.iter().any(|model| model.id == model_id) {
            Ok(agent)
        } else {
            Err(format!(
                "model {model_id} not available for agent {agent_id}"
            ))
        }
    }
}

/// Go `warningExecutableNotFound` — set when the launch command is missing.
const WARNING_EXECUTABLE_NOT_FOUND: &str = "Executable not found in PATH";

/// Mirror Go `verifyAgentExecutable`: set/clear the PATH warning based on
/// whether `command` resolves as a file or on `$PATH`.
fn verify_agent_executable(mut agent: AgentInfo) -> AgentInfo {
    if agent.command.is_empty() {
        return agent;
    }
    let exists = std::path::Path::new(&agent.command).is_file() || command_on_path(&agent.command);
    if exists {
        if agent.warning == WARNING_EXECUTABLE_NOT_FOUND {
            agent.warning.clear();
        }
    } else {
        agent.warning = WARNING_EXECUTABLE_NOT_FOUND.to_string();
    }
    agent
}

/// Best-effort `exec.LookPath` for a bare command name (or absolute path).
fn command_on_path(command: &str) -> bool {
    let path = std::path::Path::new(command);
    if path.is_absolute() || command.contains('/') || command.contains('\\') {
        return path.is_file();
    }
    let Ok(path_env) = std::env::var("PATH") else {
        return false;
    };
    for dir in std::env::split_paths(&path_env) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return true;
        }
        #[cfg(windows)]
        {
            for ext in ["exe", "cmd", "bat"] {
                let with_ext = dir.join(format!("{command}.{ext}"));
                if with_ext.is_file() {
                    return true;
                }
            }
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::AgentRegistry;
    use crate::config::{AgentInfo, AgentModel};

    fn agent(id: &str) -> AgentInfo {
        AgentInfo {
            id: id.into(),
            name: format!("Agent {id}"),
            // Real binary so verify_agent_executable does not overwrite Warning.
            command: "/bin/sh".into(),
            args: vec!["--flag".into()],
            models: vec![AgentModel::new("model-a".into(), "Model A".into())],
            warning: "warning".into(),
        }
    }

    #[test]
    fn list_is_sorted_and_omits_launch_details() {
        let registry = AgentRegistry::from_agents([agent("z"), agent("a")]);
        let listed = registry.list();
        assert_eq!(
            listed
                .iter()
                .map(|agent| agent.id.as_str())
                .collect::<Vec<_>>(),
            ["a", "z"]
        );
        assert!(listed.iter().all(|agent| agent.command.is_empty()));
        assert!(listed.iter().all(|agent| agent.args.is_empty()));
    }

    #[test]
    fn register_replaces_and_snapshots_are_independent() {
        let registry = AgentRegistry::default();
        let mut original = agent("a");
        registry.register(original.clone());
        original.args[0] = "tampered".into();
        original.models[0].id = "tampered".into();

        let mut snapshot = registry.get("a").expect("registered agent");
        assert_eq!(snapshot.args, ["--flag"]);
        snapshot.models[0].id = "changed".into();
        assert_eq!(
            registry.get("a").expect("registered agent").models[0].id,
            "model-a"
        );

        let mut replacement = agent("a");
        replacement.name = "Replacement".into();
        registry.register(replacement);
        assert_eq!(
            registry.get("a").expect("registered agent").name,
            "Replacement"
        );
    }

    #[test]
    fn resolve_and_remove_match_contract_errors() {
        let registry = AgentRegistry::from_agents([agent("a")]);
        assert_eq!(
            registry.resolve("missing", "model-a"),
            Err("agent not found: missing".into())
        );
        assert_eq!(
            registry.resolve("a", "missing"),
            Err("model missing not available for agent a".into())
        );
        assert!(registry.resolve("a", "model-a").is_ok());
        registry.remove("missing");
        registry.remove("a");
        assert!(registry.get("a").is_none());
    }
}
