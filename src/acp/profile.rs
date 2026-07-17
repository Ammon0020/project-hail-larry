//! Per-session Code / Ask / Plan prompt instructions.

use std::collections::HashMap;
use std::sync::RwLock;

use crate::interfaces::AppError;

/// Profile instruction state owned by the ACP client rather than REST handlers.
#[derive(Default)]
pub struct ProfileMiddleware {
    profiles: RwLock<HashMap<String, String>>,
}

impl ProfileMiddleware {
    /// Stores a normalized profile. Unknown values intentionally use Code mode.
    pub fn set_profile(&self, session_id: &str, profile: &str) -> Result<(), AppError> {
        self.profiles
            .write()
            .map_err(|_| AppError::internal("session profile lock poisoned"))?
            .insert(
                session_id.to_string(),
                normalize_profile(profile).to_string(),
            );
        Ok(())
    }

    /// Returns the selected profile, falling back to Code for older sessions.
    pub fn profile(&self, session_id: &str) -> Result<String, AppError> {
        Ok(self
            .profiles
            .read()
            .map_err(|_| AppError::internal("session profile lock poisoned"))?
            .get(session_id)
            .cloned()
            .unwrap_or_else(|| "Code".to_string()))
    }

    /// Removes profile state with the session.
    pub fn clear(&self, session_id: &str) {
        if let Ok(mut profiles) = self.profiles.write() {
            profiles.remove(session_id);
        }
    }

    /// Builds the profile section used as text or an embedded resource.
    pub fn instructions(&self, session_id: &str) -> Result<String, AppError> {
        let profile = self.profile(session_id)?;
        Ok(format!(
            "## Active Profile: {profile}\n\n{}",
            instructions_for(&profile)
        ))
    }
}

fn normalize_profile(profile: &str) -> &'static str {
    match profile.trim().to_ascii_lowercase().as_str() {
        "ask" => "Ask",
        "plan" => "Plan",
        _ => "Code",
    }
}

fn instructions_for(profile: &str) -> &'static str {
    match profile {
        "Ask" => {
            "You are in ASK mode. Answer questions and analyze the codebase. Do not modify files or run commands that write to disk."
        }
        "Plan" => {
            "You are in PLAN mode. Produce a detailed implementation plan and ask for explicit confirmation before writing code or running commands."
        }
        _ => {
            "You are in CODE mode. Implement requested changes, edit files, and run relevant commands as needed."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ProfileMiddleware;

    #[test]
    fn unknown_and_empty_profiles_use_code_mode() {
        let profiles = ProfileMiddleware::default();
        profiles
            .set_profile("session", "unexpected")
            .expect("set profile");

        assert!(profiles
            .instructions("session")
            .expect("profile instructions")
            .contains("CODE mode"));
    }
}
