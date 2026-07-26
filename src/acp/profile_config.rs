//! User-editable profile configuration loaded from `~/.local-agent/profiles.json`.
//!
//! Missing file → built-in Code/Ask/Plan defaults (common case, not an error).
//! Parse/validation failures fail loudly; callers decide whether to fall back.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::config::Config;

/// On-disk config file name under the resolved state directory.
const CONFIG_FILE_NAME: &str = "profiles.json";

/// Reject configs larger than this before JSON parsing (`DoS` guard).
///
/// Also used as the REST `PUT /api/profiles` body-size cap so on-disk and
/// over-the-wire limits stay aligned.
pub const MAX_FILE_BYTES: u64 = 256 * 1024;

/// Owner read/write only — config has no secrets but stays consistent with
/// other state files under `~/.local-agent`.
const CONFIG_FILE_PERM: u32 = 0o600;

/// Maximum number of profiles in one config file.
const MAX_PROFILES: usize = 50;

/// Maximum characters in a single profile's instruction text.
const MAX_INSTRUCTION_CHARS: usize = 16 * 1024;

/// Maximum characters in a profile label shown in UI / prompt headers.
const MAX_LABEL_CHARS: usize = 100;

/// Maximum characters in a single tool name entry.
const MAX_TOOL_NAME_CHARS: usize = 200;

/// Characters that must not appear in tool names (path / shell injection surface).
const UNSAFE_TOOL_NAME_CHARS: &[char] = &['/', '\\', ';', '|', '&', '`', '$', '(', ')', '<', '>'];

/// One named agent profile: prompt instructions plus optional MCP server policy.
///
/// # MCP server semantics
///
/// An omitted `mcpServers` field means all enabled configured MCP servers are
/// available. An explicit empty list means no MCP servers are available. The
/// ACP session setup carries servers, not per-tool subsets, so a profile always
/// selects complete MCP servers.
///
/// `legacyTools` retains the old `tools` data only so Settings can explain the
/// migration. It is never interpreted as MCP server names. Profiles with a
/// non-empty legacy list fail closed (no MCP servers) until the user selects
/// explicit `mcpServers`; see `ProfileMiddleware::mcp_servers_for_session`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Profile {
    /// Human-readable name used in prompt headers and UI (e.g. `"Code"`).
    pub label: String,
    /// Instruction text injected into the prompt for this profile.
    pub instructions: String,
    /// Optional complete-MCP-server allowlist. `None` = all enabled servers;
    /// `Some(vec![])` = no MCP servers.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mcp_servers: Option<Vec<String>>,
    /// Read-only migration information from the former `tools` field. The
    /// serialized name makes migration visible to Settings while accepting old
    /// on-disk configs without silently treating tool names as server names.
    #[serde(
        rename = "legacyTools",
        alias = "tools",
        default,
        skip_serializing_if = "Option::is_none"
    )]
    pub legacy_tools: Option<Vec<String>>,
}

/// Root envelope for `profiles.json`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProfileConfig {
    /// Profile id → definition. Ids are map keys (`[a-zA-Z0-9_-]+`).
    pub profiles: BTreeMap<String, Profile>,
    /// Id used when the client omits or sends an unknown profile.
    pub default_profile_id: String,
}

impl Default for ProfileConfig {
    fn default() -> Self {
        Self::builtin_defaults()
    }
}

impl ProfileConfig {
    /// Built-in Code / Ask / Plan profiles matching today's hardcoded strings.
    ///
    /// Labels are title-case so `## Active Profile: {label}` stays byte-identical
    /// to the previous `ProfileMiddleware` output. Ids are lowercase
    /// (`code` / `ask` / `plan`); `default_profile_id` is `"code"`.
    /// `mcpServers` is omitted on all built-ins → all enabled MCP servers.
    #[must_use]
    pub fn builtin_defaults() -> Self {
        let mut profiles = BTreeMap::new();
        profiles.insert(
            "code".to_string(),
            Profile {
                label: "Code".to_string(),
                instructions: BUILTIN_CODE_INSTRUCTIONS.to_string(),
                mcp_servers: None,
                legacy_tools: None,
            },
        );
        profiles.insert(
            "ask".to_string(),
            Profile {
                label: "Ask".to_string(),
                instructions: BUILTIN_ASK_INSTRUCTIONS.to_string(),
                mcp_servers: None,
                legacy_tools: None,
            },
        );
        profiles.insert(
            "plan".to_string(),
            Profile {
                label: "Plan".to_string(),
                instructions: BUILTIN_PLAN_INSTRUCTIONS.to_string(),
                mcp_servers: None,
                legacy_tools: None,
            },
        );
        Self {
            profiles,
            default_profile_id: "code".to_string(),
        }
    }

    /// Returns `<state_dir>/profiles.json`.
    ///
    /// # Errors
    ///
    /// Returns an error if the default state directory cannot be resolved.
    pub fn path() -> Result<PathBuf, ProfileConfigError> {
        Ok(Config::resolved_state_dir()?.join(CONFIG_FILE_NAME))
    }

    /// Loads from the default state-dir path.
    ///
    /// Missing file → [`Self::builtin_defaults`] (no error).
    ///
    /// # Errors
    /// Returns an error if the default path cannot be resolved or the file cannot be loaded.
    pub fn load_default() -> Result<Self, ProfileConfigError> {
        Self::load(&Self::path()?)
    }

    /// Loads a config from `path`.
    ///
    /// Missing file → built-in defaults. I/O (other than not-found), parse, and
    /// validation errors are returned and must not be swallowed by callers that
    /// need to surface bad config.
    ///
    /// # Errors
    /// Returns an error on I/O failure (other than not-found), parse errors, or validation failures.
    pub fn load(path: &Path) -> Result<Self, ProfileConfigError> {
        match fs::metadata(path) {
            Ok(meta) => {
                let len = meta.len();
                if len > MAX_FILE_BYTES {
                    return Err(ProfileConfigError::FileTooLarge {
                        size: len,
                        max: MAX_FILE_BYTES,
                    });
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                // Common case for existing installs: no custom profiles yet.
                return Ok(Self::builtin_defaults());
            }
            Err(error) => return Err(error.into()),
        }

        let raw = fs::read(path)?;
        // Defense in depth if the file grew between stat and read.
        if raw.len() as u64 > MAX_FILE_BYTES {
            return Err(ProfileConfigError::FileTooLarge {
                size: raw.len() as u64,
                max: MAX_FILE_BYTES,
            });
        }
        Self::parse(&raw)
    }

    /// Parses and validates a raw JSON document.
    ///
    /// # Errors
    ///
    /// Returns an error if the document cannot be parsed or fails validation.
    pub fn parse(raw: &[u8]) -> Result<Self, ProfileConfigError> {
        let config: Self = serde_json::from_slice(raw).map_err(ProfileConfigError::Json)?;
        config.validate()?;
        Ok(config)
    }

    /// Validates, serializes, and atomically writes to the default state-dir path.
    ///
    /// Used by `PUT /api/profiles`. Does not update in-memory middleware; callers
    /// must [`super::profile::ProfileMiddleware::replace_config`] (or `reload`)
    /// after a successful save.
    ///
    /// # Errors
    /// Returns an error if the default path cannot be resolved, validation fails, serialization fails, or the atomic write fails.
    pub fn save(&self) -> Result<(), ProfileConfigError> {
        self.save_to(&Self::path()?)
    }

    /// Validates, serializes, and atomically writes `path` (temp + rename, mode 0600).
    ///
    /// Fails before touching the destination when validation fails or the
    /// serialized payload exceeds [`MAX_FILE_BYTES`].
    ///
    /// # Errors
    /// Returns an error if validation fails, serialization fails, the payload exceeds [`MAX_FILE_BYTES`], or the atomic write fails.
    pub fn save_to(&self, path: &Path) -> Result<(), ProfileConfigError> {
        self.validate()?;
        let data = serde_json::to_vec_pretty(self).map_err(ProfileConfigError::Json)?;
        if data.len() as u64 > MAX_FILE_BYTES {
            return Err(ProfileConfigError::FileTooLarge {
                size: data.len() as u64,
                max: MAX_FILE_BYTES,
            });
        }
        crate::fsutil::atomic_write(path, &data, Some(CONFIG_FILE_PERM))?;
        Ok(())
    }

    /// Validates structural and security constraints after deserialization.
    ///
    /// # Errors
    /// Returns an error if profiles are empty/too many, the default id is missing, or any profile id/label/instructions/tool/server name is invalid.
    pub fn validate(&self) -> Result<(), ProfileConfigError> {
        if self.profiles.is_empty() {
            return Err(ProfileConfigError::Validation(
                "profiles map must not be empty".to_string(),
            ));
        }
        if self.profiles.len() > MAX_PROFILES {
            return Err(ProfileConfigError::TooManyProfiles {
                count: self.profiles.len(),
                max: MAX_PROFILES,
            });
        }
        if self.default_profile_id.is_empty() {
            return Err(ProfileConfigError::Validation(
                "defaultProfileId must not be empty".to_string(),
            ));
        }
        if !self.profiles.contains_key(&self.default_profile_id) {
            return Err(ProfileConfigError::DefaultProfileMissing(
                self.default_profile_id.clone(),
            ));
        }

        for (id, profile) in &self.profiles {
            validate_profile_id(id)?;
            validate_label(&profile.label, id)?;
            validate_instructions(&profile.instructions, id)?;
            for tool in profile.legacy_tools.as_deref().unwrap_or_default() {
                validate_tool_name(tool, id)?;
            }
            if let Some(servers) = &profile.mcp_servers {
                for server in servers {
                    validate_mcp_server_name(server, id)?;
                }
            }
        }
        Ok(())
    }

    /// Resolves a client-supplied profile string to a known profile id.
    ///
    /// Match is case-insensitive against map keys so UI values like `"Code"`
    /// resolve to the built-in `"code"` id. Unknown or blank input →
    /// `default_profile_id`.
    #[must_use]
    pub fn normalize_profile_id(&self, profile: &str) -> String {
        let trimmed = profile.trim();
        if trimmed.is_empty() {
            return self.default_profile_id.clone();
        }
        if let Some(id) = self
            .profiles
            .keys()
            .find(|id| id.eq_ignore_ascii_case(trimmed))
        {
            return id.clone();
        }
        self.default_profile_id.clone()
    }

    /// Looks up a profile by id (exact key). Falls back to built-in defaults
    /// when the id is missing from this config (defensive for racey reload).
    #[must_use]
    pub fn profile(&self, id: &str) -> Profile {
        if let Some(profile) = self.profiles.get(id) {
            return profile.clone();
        }
        let builtins = Self::builtin_defaults();
        if let Some(profile) = builtins.profiles.get(id) {
            return profile.clone();
        }
        // Built-ins are constructed with default_profile_id present; fall back to
        // CODE instructions if that invariant is ever broken in tests.
        builtins
            .profiles
            .get(&builtins.default_profile_id)
            .cloned()
            .unwrap_or(Profile {
                label: "Code".to_string(),
                instructions: BUILTIN_CODE_INSTRUCTIONS.to_string(),
                mcp_servers: None,
                legacy_tools: None,
            })
    }

    /// Rejects explicit server selections that do not exist in `mcp.json`.
    /// Omitted `mcpServers` and disabled configured servers are valid: the
    /// former means all enabled servers, and the latter may be enabled later.
    ///
    /// # Errors
    ///
    /// Returns an error if any profile references an unknown MCP server.
    pub fn validate_mcp_servers_against<'a>(
        &self,
        configured_server_names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ProfileConfigError> {
        let configured: std::collections::BTreeSet<&str> =
            configured_server_names.into_iter().collect();
        for (profile_id, profile) in &self.profiles {
            for server in profile.mcp_servers.as_deref().unwrap_or_default() {
                if !configured.contains(server.as_str()) {
                    return Err(ProfileConfigError::UnknownMcpServer {
                        profile_id: profile_id.clone(),
                        server: server.clone(),
                    });
                }
            }
        }
        Ok(())
    }

    /// Validates the explicit server selection for one resolved profile.
    ///
    /// # Errors
    ///
    /// Returns an error if the resolved profile references an unknown MCP server.
    pub fn validate_profile_mcp_servers_against<'a>(
        &self,
        profile_id: &str,
        configured_server_names: impl IntoIterator<Item = &'a str>,
    ) -> Result<(), ProfileConfigError> {
        let configured: std::collections::BTreeSet<&str> =
            configured_server_names.into_iter().collect();
        let profile = self.profile(profile_id);
        for server in profile.mcp_servers.as_deref().unwrap_or_default() {
            if !configured.contains(server.as_str()) {
                return Err(ProfileConfigError::UnknownMcpServer {
                    profile_id: profile_id.to_string(),
                    server: server.clone(),
                });
            }
        }
        Ok(())
    }
}

/// Instruction text must stay byte-identical to the previous hardcoded
/// `instructions_for` output in `profile.rs`.
const BUILTIN_CODE_INSTRUCTIONS: &str = "You are in CODE mode. Implement requested changes, edit files, and run relevant commands as needed.";
const BUILTIN_ASK_INSTRUCTIONS: &str = "You are in ASK mode. Answer questions and analyze the codebase. Do not modify files or run commands that write to disk.";
const BUILTIN_PLAN_INSTRUCTIONS: &str = "You are in PLAN mode. Produce a detailed implementation plan and ask for explicit confirmation before writing code or running commands.";

fn validate_profile_id(id: &str) -> Result<(), ProfileConfigError> {
    if id.is_empty() {
        return Err(ProfileConfigError::InvalidProfileId(id.to_string()));
    }
    let valid = id
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-');
    if !valid {
        return Err(ProfileConfigError::InvalidProfileId(id.to_string()));
    }
    Ok(())
}

fn validate_label(label: &str, profile_id: &str) -> Result<(), ProfileConfigError> {
    if label.is_empty() {
        return Err(ProfileConfigError::Validation(format!(
            "profile `{profile_id}`: label must not be empty"
        )));
    }
    if label.chars().count() > MAX_LABEL_CHARS {
        return Err(ProfileConfigError::LabelTooLong {
            profile_id: profile_id.to_string(),
            len: label.chars().count(),
            max: MAX_LABEL_CHARS,
        });
    }
    Ok(())
}

fn validate_instructions(instructions: &str, profile_id: &str) -> Result<(), ProfileConfigError> {
    if instructions.chars().count() > MAX_INSTRUCTION_CHARS {
        return Err(ProfileConfigError::InstructionsTooLong {
            profile_id: profile_id.to_string(),
            len: instructions.chars().count(),
            max: MAX_INSTRUCTION_CHARS,
        });
    }
    Ok(())
}

/// Shared empty/oversized/forbidden-character/whitespace check for tool and
/// MCP-server names. Returns the failure reason so each caller can wrap it in
/// its own error variant (`UnsafeToolName` / `UnsafeMcpServerName`).
fn validate_name_chars(name: &str) -> Result<(), String> {
    if name.is_empty() || name.chars().all(char::is_whitespace) {
        return Err("name must not be empty or whitespace-only".to_string());
    }
    if name.chars().count() > MAX_TOOL_NAME_CHARS {
        return Err(format!("name exceeds {MAX_TOOL_NAME_CHARS} characters"));
    }
    if let Some(ch) = name.chars().find(|c| UNSAFE_TOOL_NAME_CHARS.contains(c)) {
        return Err(format!("name contains forbidden character `{ch}`"));
    }
    // Reject embedded whitespace so names cannot smuggle multiple tokens.
    if name.chars().any(char::is_whitespace) {
        return Err("name must not contain whitespace".to_string());
    }
    Ok(())
}

fn validate_tool_name(name: &str, profile_id: &str) -> Result<(), ProfileConfigError> {
    validate_name_chars(name).map_err(|reason| ProfileConfigError::UnsafeToolName {
        profile_id: profile_id.to_string(),
        tool: name.to_string(),
        reason,
    })
}

fn validate_mcp_server_name(name: &str, profile_id: &str) -> Result<(), ProfileConfigError> {
    validate_name_chars(name).map_err(|reason| ProfileConfigError::UnsafeMcpServerName {
        profile_id: profile_id.to_string(),
        server: name.to_string(),
        reason,
    })
}

/// Errors from profile config path resolution, I/O, JSON, and validation.
#[derive(Debug, Error)]
pub enum ProfileConfigError {
    #[error("profile config I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("invalid profile config JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("profile config file too large: {size} bytes (max {max})")]
    FileTooLarge { size: u64, max: u64 },
    #[error("too many profiles: {count} (max {max})")]
    TooManyProfiles { count: usize, max: usize },
    #[error("defaultProfileId `{0}` is not present in profiles")]
    DefaultProfileMissing(String),
    #[error(
        "invalid profile id `{0}`: must match [a-zA-Z0-9_-]+ (no spaces or special characters)"
    )]
    InvalidProfileId(String),
    #[error("profile `{profile_id}`: label too long ({len} chars, max {max})")]
    LabelTooLong {
        profile_id: String,
        len: usize,
        max: usize,
    },
    #[error("profile `{profile_id}`: instructions too long ({len} chars, max {max})")]
    InstructionsTooLong {
        profile_id: String,
        len: usize,
        max: usize,
    },
    #[error("profile `{profile_id}`: unsafe tool name `{tool}`: {reason}")]
    UnsafeToolName {
        profile_id: String,
        tool: String,
        reason: String,
    },
    #[error("profile `{profile_id}`: unsafe MCP server name `{server}`: {reason}")]
    UnsafeMcpServerName {
        profile_id: String,
        server: String,
        reason: String,
    },
    #[error("profile `{profile_id}` selects MCP server `{server}`, which is not configured")]
    UnknownMcpServer { profile_id: String, server: String },
    #[error("invalid profile config: {0}")]
    Validation(String),
    #[error(transparent)]
    Config(#[from] crate::config::ConfigError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write as _;
    use std::io::Write;

    fn write_temp_config(raw: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        let mut file = fs::File::create(&path).expect("create config");
        file.write_all(raw.as_bytes()).expect("write config");
        (dir, path)
    }

    #[test]
    fn missing_file_returns_builtin_defaults() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("profiles.json");
        let config = ProfileConfig::load(&path).expect("missing file is defaults");
        assert_eq!(config, ProfileConfig::builtin_defaults());
        assert_eq!(config.default_profile_id, "code");
        assert_eq!(
            config.profile("code").instructions,
            BUILTIN_CODE_INSTRUCTIONS
        );
        assert_eq!(config.profile("ask").instructions, BUILTIN_ASK_INSTRUCTIONS);
        assert_eq!(
            config.profile("plan").instructions,
            BUILTIN_PLAN_INSTRUCTIONS
        );
        // Labels preserve prior prompt header casing.
        assert_eq!(config.profile("code").label, "Code");
        assert_eq!(config.profile("code").mcp_servers, None);
    }

    #[test]
    fn builtin_instruction_strings_match_legacy_hardcoded_text() {
        // Guard against accidental drift from the pre-config profile.rs strings.
        assert_eq!(
            BUILTIN_CODE_INSTRUCTIONS,
            "You are in CODE mode. Implement requested changes, edit files, and run relevant commands as needed."
        );
        assert_eq!(
            BUILTIN_ASK_INSTRUCTIONS,
            "You are in ASK mode. Answer questions and analyze the codebase. Do not modify files or run commands that write to disk."
        );
        assert_eq!(
            BUILTIN_PLAN_INSTRUCTIONS,
            "You are in PLAN mode. Produce a detailed implementation plan and ask for explicit confirmation before writing code or running commands."
        );
    }

    #[test]
    fn valid_custom_profile_loads() {
        let raw = r#"{
          "profiles": {
            "code": {
              "label": "Code",
              "instructions": "code-instructions",
              "tools": []
            },
            "review": {
              "label": "Review",
              "instructions": "custom-review-instructions",
              "tools": ["read_file", "grep"]
            }
          },
          "defaultProfileId": "code"
        }"#;
        let (_dir, path) = write_temp_config(raw);
        let config = ProfileConfig::load(&path).expect("valid config");
        assert_eq!(
            config.profile("review").instructions,
            "custom-review-instructions"
        );
        assert_eq!(
            config.profile("review").legacy_tools,
            Some(vec!["read_file".to_string(), "grep".to_string()])
        );
    }

    #[test]
    fn save_to_writes_atomically_and_round_trips() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        let config = ProfileConfig::builtin_defaults();
        config.save_to(&path).expect("save");
        let loaded = ProfileConfig::load(&path).expect("reload");
        assert_eq!(loaded, config);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(&path).expect("meta").permissions().mode() & 0o777;
            assert_eq!(mode, 0o600, "profiles.json must be owner-only");
        }
        // Invalid config must not replace an existing good file.
        let bad = ProfileConfig {
            profiles: BTreeMap::new(),
            default_profile_id: "missing".to_string(),
        };
        assert!(bad.save_to(&path).is_err());
        let still = ProfileConfig::load(&path).expect("still good");
        assert_eq!(still, config);
    }

    #[test]
    fn malformed_json_errors() {
        let err = ProfileConfig::parse(br"{ not json").expect_err("malformed");
        assert!(matches!(err, ProfileConfigError::Json(_)));
    }

    #[test]
    fn unknown_field_is_rejected() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x", "tools": [], "extra": 1 }
          },
          "defaultProfileId": "code"
        }"#;
        let err = ProfileConfig::parse(raw.as_bytes()).expect_err("unknown field");
        assert!(matches!(err, ProfileConfigError::Json(_)));
    }

    #[test]
    fn unknown_root_field_is_rejected() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x", "tools": [] }
          },
          "defaultProfileId": "code",
          "surprise": true
        }"#;
        let err = ProfileConfig::parse(raw.as_bytes()).expect_err("unknown root field");
        assert!(matches!(err, ProfileConfigError::Json(_)));
    }

    #[test]
    fn oversized_file_errors() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join(CONFIG_FILE_NAME);
        // MAX_FILE_BYTES is 256 KiB, well within usize on all supported targets.
        #[allow(clippy::cast_possible_truncation)]
        let big = vec![b'a'; (MAX_FILE_BYTES as usize) + 1];
        fs::write(&path, big).expect("write oversized");
        let err = ProfileConfig::load(&path).expect_err("oversize");
        assert!(matches!(err, ProfileConfigError::FileTooLarge { .. }));
    }

    #[test]
    fn too_many_profiles_errors() {
        let mut profiles = String::from("{");
        for i in 0..=MAX_PROFILES {
            if i > 0 {
                profiles.push(',');
            }
            let _ = write!(
                profiles,
                r#""p{i}":{{"label":"L{i}","instructions":"i","tools":[]}}"#
            );
        }
        profiles.push('}');
        let raw = format!(r#"{{"profiles":{profiles},"defaultProfileId":"p0"}}"#);
        let err = ProfileConfig::parse(raw.as_bytes()).expect_err("too many");
        assert!(matches!(err, ProfileConfigError::TooManyProfiles { .. }));
    }

    #[test]
    fn instruction_too_long_errors() {
        let long = "x".repeat(MAX_INSTRUCTION_CHARS + 1);
        let raw = format!(
            r#"{{
              "profiles": {{
                "code": {{ "label": "Code", "instructions": "{long}", "tools": [] }}
              }},
              "defaultProfileId": "code"
            }}"#
        );
        let err = ProfileConfig::parse(raw.as_bytes()).expect_err("instructions too long");
        assert!(matches!(
            err,
            ProfileConfigError::InstructionsTooLong { .. }
        ));
    }

    #[test]
    fn unsafe_tool_names_error() {
        for tool in [
            "../etc", "a;b", "a|b", "a&b", "a`b", "a$b", "a(b)", "a<b", " ", "a b",
        ] {
            let raw = format!(
                r#"{{
                  "profiles": {{
                    "code": {{ "label": "Code", "instructions": "x", "tools": ["{tool}"] }}
                  }},
                  "defaultProfileId": "code"
                }}"#
            );
            let err = ProfileConfig::parse(raw.as_bytes())
                .expect_err(&format!("expected unsafe tool error for {tool:?}"));
            assert!(
                matches!(err, ProfileConfigError::UnsafeToolName { .. }),
                "tool {tool:?} => {err}"
            );
        }
    }

    #[test]
    fn invalid_profile_id_errors() {
        let raw = r#"{
          "profiles": {
            "bad id": { "label": "Bad", "instructions": "x", "tools": [] }
          },
          "defaultProfileId": "bad id"
        }"#;
        let err = ProfileConfig::parse(raw.as_bytes()).expect_err("invalid id");
        assert!(matches!(err, ProfileConfigError::InvalidProfileId(_)));
    }

    #[test]
    fn default_profile_id_must_exist() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x", "tools": [] }
          },
          "defaultProfileId": "missing"
        }"#;
        let err = ProfileConfig::parse(raw.as_bytes()).expect_err("missing default");
        assert!(matches!(err, ProfileConfigError::DefaultProfileMissing(_)));
    }

    #[test]
    fn normalize_unknown_uses_default() {
        let config = ProfileConfig::builtin_defaults();
        assert_eq!(config.normalize_profile_id("unexpected"), "code");
        assert_eq!(config.normalize_profile_id(""), "code");
        assert_eq!(config.normalize_profile_id("  "), "code");
        assert_eq!(config.normalize_profile_id("Ask"), "ask");
        assert_eq!(config.normalize_profile_id("CODE"), "code");
        assert_eq!(config.normalize_profile_id("plan"), "plan");
    }

    #[test]
    fn omitted_mcp_servers_allows_all_enabled_servers() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x" }
          },
          "defaultProfileId": "code"
        }"#;
        let config = ProfileConfig::parse(raw.as_bytes()).expect("mcpServers optional");
        assert_eq!(config.profile("code").mcp_servers, None);
    }

    #[test]
    fn explicit_empty_mcp_servers_means_no_mcp_servers() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x", "mcpServers": [] }
          },
          "defaultProfileId": "code"
        }"#;
        let config = ProfileConfig::parse(raw.as_bytes()).expect("valid config");
        assert_eq!(config.profile("code").mcp_servers, Some(Vec::new()));
    }

    #[test]
    fn legacy_tools_are_preserved_without_becoming_server_names() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x", "tools": ["read_file"] }
          },
          "defaultProfileId": "code"
        }"#;
        let config = ProfileConfig::parse(raw.as_bytes()).expect("legacy config");
        let profile = config.profile("code");
        assert_eq!(profile.mcp_servers, None);
        assert_eq!(profile.legacy_tools, Some(vec!["read_file".to_string()]));
        let json = serde_json::to_value(config).expect("serialize");
        assert_eq!(json["profiles"]["code"]["legacyTools"][0], "read_file");
        assert!(json["profiles"]["code"].get("tools").is_none());
    }

    #[test]
    fn explicit_servers_must_exist_when_validated_against_mcp_config() {
        let raw = r#"{
          "profiles": {
            "code": { "label": "Code", "instructions": "x", "mcpServers": ["context7"] }
          },
          "defaultProfileId": "code"
        }"#;
        let config = ProfileConfig::parse(raw.as_bytes()).expect("valid config");
        assert!(config
            .validate_mcp_servers_against(["context7", "workspace-read"])
            .is_ok());
        assert!(matches!(
            config.validate_mcp_servers_against(["workspace-read"]),
            Err(ProfileConfigError::UnknownMcpServer { .. })
        ));
    }
}
