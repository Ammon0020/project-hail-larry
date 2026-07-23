//! Per-session profile selection and prompt instruction injection.
//!
//! Profile definitions come from [`profile_config::ProfileConfig`]
//! (`~/.local-agent/profiles.json`), with built-in Code/Ask/Plan fallbacks.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use super::profile_config::ProfileConfig;
use crate::interfaces::AppError;

/// Profile instruction state owned by the ACP client rather than REST handlers.
pub struct ProfileMiddleware {
    /// session_id → normalized profile id from config.
    sessions: RwLock<HashMap<String, String>>,
    /// Loaded profile definitions; reloadable after REST writes (S-PROF-REST).
    config: Arc<RwLock<ProfileConfig>>,
}

impl Default for ProfileMiddleware {
    fn default() -> Self {
        Self::from_config(load_config_at_startup())
    }
}

impl ProfileMiddleware {
    /// Builds middleware around an already-loaded config (tests / custom wiring).
    #[must_use]
    pub fn from_config(config: ProfileConfig) -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            config: Arc::new(RwLock::new(config)),
        }
    }

    /// Stores a normalized profile id for the session.
    ///
    /// Unknown values map to the config's `defaultProfileId` (built-in: `"code"`).
    pub fn set_profile(&self, session_id: &str, profile: &str) -> Result<(), AppError> {
        let normalized = self.normalize_profile(profile)?;
        self.sessions
            .write()
            .map_err(|_| AppError::internal("session profile lock poisoned"))?
            .insert(session_id.to_string(), normalized);
        Ok(())
    }

    /// Returns the selected profile id, falling back to the config default.
    pub fn profile(&self, session_id: &str) -> Result<String, AppError> {
        let sessions = self
            .sessions
            .read()
            .map_err(|_| AppError::internal("session profile lock poisoned"))?;
        if let Some(id) = sessions.get(session_id) {
            return Ok(id.clone());
        }
        self.default_profile_id()
    }

    /// Removes profile state with the session.
    pub fn clear(&self, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.write() {
            sessions.remove(session_id);
        }
    }

    /// Builds the profile section used as text or an embedded resource.
    ///
    /// Format: `## Active Profile: {label}\n\n{instructions}` where `label` and
    /// `instructions` come from the loaded config (built-in labels are
    /// `"Code"` / `"Ask"` / `"Plan"` so existing prompts stay byte-identical).
    pub fn instructions(&self, session_id: &str) -> Result<String, AppError> {
        let profile_id = self.profile(session_id)?;
        let config = self
            .config
            .read()
            .map_err(|_| AppError::internal("profile config lock poisoned"))?;
        let profile = config.profile(&profile_id);
        Ok(format!(
            "## Active Profile: {}\n\n{}",
            profile.label, profile.instructions
        ))
    }

    /// Replaces the in-memory config (REST PUT and tests that already parsed).
    pub fn replace_config(&self, config: ProfileConfig) -> Result<(), AppError> {
        *self
            .config
            .write()
            .map_err(|_| AppError::internal("profile config lock poisoned"))? = config;
        Ok(())
    }

    /// Returns a snapshot of the full loaded config (`GET /api/profiles`).
    pub fn config(&self) -> Result<ProfileConfig, AppError> {
        self.config
            .read()
            .map(|config| config.clone())
            .map_err(|_| AppError::internal("profile config lock poisoned"))
    }

    /// MCP server policy for a session's active profile.
    ///
    /// `None` means all enabled configured servers; an empty `Vec` means none.
    /// A profile that still has a non-empty former tool whitelist has no safe
    /// server-level equivalent, so it fails closed until Settings migrates it.
    pub fn mcp_servers_for_session(
        &self,
        session_id: &str,
    ) -> Result<Option<Vec<String>>, AppError> {
        let profile_id = self.profile(session_id)?;
        let config = self
            .config
            .read()
            .map_err(|_| AppError::internal("profile config lock poisoned"))?;
        let profile = config.profile(&profile_id);
        if profile.mcp_servers.is_none()
            && profile
                .legacy_tools
                .as_ref()
                .is_some_and(|tools| !tools.is_empty())
        {
            return Ok(Some(Vec::new()));
        }
        Ok(profile.mcp_servers)
    }

    fn normalize_profile(&self, profile: &str) -> Result<String, AppError> {
        let config = self
            .config
            .read()
            .map_err(|_| AppError::internal("profile config lock poisoned"))?;
        Ok(config.normalize_profile_id(profile))
    }

    fn default_profile_id(&self) -> Result<String, AppError> {
        let config = self
            .config
            .read()
            .map_err(|_| AppError::internal("profile config lock poisoned"))?;
        Ok(config.default_profile_id.clone())
    }
}

/// Loads `profiles.json` at daemon/pipeline construction.
///
/// Missing file → built-ins (quiet). Corrupt/invalid file → log loudly and
/// still start with built-ins so a bad edit cannot take down the daemon.
fn load_config_at_startup() -> ProfileConfig {
    match ProfileConfig::load_default() {
        Ok(config) => {
            tracing::debug!(
                profiles = config.profiles.len(),
                default = %config.default_profile_id,
                "loaded profile config"
            );
            config
        }
        Err(error) => {
            tracing::error!(
                error = %error,
                "failed to load profiles.json; falling back to built-in defaults"
            );
            ProfileConfig::builtin_defaults()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::profile_config::{Profile, ProfileConfig};
    use std::collections::BTreeMap;

    #[test]
    fn unknown_and_empty_profiles_use_code_mode() {
        let profiles = ProfileMiddleware::from_config(ProfileConfig::builtin_defaults());
        profiles
            .set_profile("session", "unexpected")
            .expect("set profile");

        let text = profiles
            .instructions("session")
            .expect("profile instructions");
        assert!(text.contains("CODE mode"));
        assert!(text.starts_with("## Active Profile: Code\n\n"));
        assert_eq!(profiles.profile("session").expect("id"), "code");

        profiles
            .set_profile("empty", "")
            .expect("set empty profile");
        assert_eq!(profiles.profile("empty").expect("id"), "code");
    }

    #[test]
    fn builtin_instruction_output_is_byte_identical_to_legacy() {
        let profiles = ProfileMiddleware::from_config(ProfileConfig::builtin_defaults());
        for (input, header_label, needle) in [
            ("Code", "Code", "CODE mode"),
            ("ask", "Ask", "ASK mode"),
            ("PLAN", "Plan", "PLAN mode"),
        ] {
            profiles.set_profile("s", input).expect("set profile");
            let text = profiles.instructions("s").expect("instructions");
            let expected_prefix = format!("## Active Profile: {header_label}\n\n");
            assert!(text.starts_with(&expected_prefix), "input {input}: {text}");
            assert!(text.contains(needle), "input {input}: {text}");
        }

        // Exact full strings for the three built-ins.
        profiles.set_profile("s", "code").expect("set");
        assert_eq!(
            profiles.instructions("s").expect("code"),
            "## Active Profile: Code\n\nYou are in CODE mode. Implement requested changes, edit files, and run relevant commands as needed."
        );
        profiles.set_profile("s", "ask").expect("set");
        assert_eq!(
            profiles.instructions("s").expect("ask"),
            "## Active Profile: Ask\n\nYou are in ASK mode. Answer questions and analyze the codebase. Do not modify files or run commands that write to disk."
        );
        profiles.set_profile("s", "plan").expect("set");
        assert_eq!(
            profiles.instructions("s").expect("plan"),
            "## Active Profile: Plan\n\nYou are in PLAN mode. Produce a detailed implementation plan and ask for explicit confirmation before writing code or running commands."
        );
    }

    #[test]
    fn custom_profile_instructions_are_returned() {
        let mut profiles_map = BTreeMap::new();
        profiles_map.insert(
            "code".to_string(),
            Profile {
                label: "Code".to_string(),
                instructions: "code-fallback".to_string(),
                mcp_servers: None,
                legacy_tools: None,
            },
        );
        profiles_map.insert(
            "review".to_string(),
            Profile {
                label: "Review".to_string(),
                instructions: "custom-review-body".to_string(),
                mcp_servers: Some(vec!["workspace-read".to_string()]),
                legacy_tools: None,
            },
        );
        let config = ProfileConfig {
            profiles: profiles_map,
            default_profile_id: "code".to_string(),
        };
        let middleware = ProfileMiddleware::from_config(config);
        middleware
            .set_profile("session", "review")
            .expect("set custom");
        assert_eq!(
            middleware.instructions("session").expect("instructions"),
            "## Active Profile: Review\n\ncustom-review-body"
        );
        let snapshot = middleware.config().expect("snapshot");
        assert_eq!(
            snapshot.profiles["review"].mcp_servers,
            Some(vec!["workspace-read".to_string()])
        );
    }

    #[test]
    fn replace_config_updates_resolution() {
        let middleware = ProfileMiddleware::from_config(ProfileConfig::builtin_defaults());
        let mut profiles_map = BTreeMap::new();
        profiles_map.insert(
            "focus".to_string(),
            Profile {
                label: "Focus".to_string(),
                instructions: "focus-only".to_string(),
                mcp_servers: None,
                legacy_tools: None,
            },
        );
        middleware
            .replace_config(ProfileConfig {
                profiles: profiles_map,
                default_profile_id: "focus".to_string(),
            })
            .expect("replace");
        middleware.set_profile("s", "unknown").expect("set unknown");
        assert_eq!(middleware.profile("s").expect("id"), "focus");
        assert!(middleware
            .instructions("s")
            .expect("text")
            .contains("focus-only"));
    }

    #[test]
    fn legacy_tool_policy_fails_closed_until_servers_are_selected() {
        let config = ProfileConfig::parse(
            br#"{
              "profiles": {
                "code": { "label": "Code", "instructions": "x", "tools": ["read_file"] }
              },
              "defaultProfileId": "code"
            }"#,
        )
        .expect("legacy profile config");
        let middleware = ProfileMiddleware::from_config(config);
        assert_eq!(
            middleware
                .mcp_servers_for_session("session")
                .expect("policy"),
            Some(Vec::new())
        );
    }
}
