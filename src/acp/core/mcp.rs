use std::path::Path;

use agent_client_protocol::schema::v1::{McpCapabilities, McpServer};

/// Load enabled MCP servers for session/new, filtered by agent capabilities
/// and the active profile's complete-server allowlist.
///
/// Missing path / missing file / parse errors yield an empty list (Go parity:
/// MCP is additive and must not block session creation).
///
/// # Profile filtering
///
/// Omitted `mcpServers` = all capability-filtered servers; an explicit list
/// attaches only named servers (including an explicit empty list for none).
pub(super) async fn load_session_mcp_servers(
    path: Option<&Path>,
    caps: &McpCapabilities,
    profiles: &super::super::profile::ProfileMiddleware,
    session_id: &str,
) -> Vec<McpServer> {
    let Some(path) = path else {
        return Vec::new();
    };
    let file = match crate::mcp::File::load(path) {
        Ok(file) => file,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "loading mcp config; continuing without mcp servers"
            );
            return Vec::new();
        }
    };
    let servers = match file.to_acp(caps) {
        Ok(servers) => servers,
        Err(error) => {
            tracing::warn!(
                path = %path.display(),
                %error,
                "converting mcp config; continuing without mcp servers"
            );
            return Vec::new();
        }
    };
    if servers.is_empty() {
        return servers;
    }

    let allowlist = profiles.mcp_servers_for_session(session_id).unwrap_or(None);
    let servers = crate::mcp::filter_servers_by_name(servers, allowlist.as_deref());

    if !servers.is_empty() {
        tracing::debug!(
            path = %path.display(),
            count = servers.len(),
            allowlist_len = allowlist.as_ref().map_or(0, Vec::len),
            "attaching MCP servers to session/new"
        );
    }
    servers
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    #[tokio::test]
    async fn load_session_mcp_servers_attaches_enabled_stdio() {
        use agent_client_protocol::schema::v1::{McpCapabilities, McpServer};
        use std::fs;

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            r#"{
  "version": 1,
  "mcpServers": {
    "echo": {
      "command": "echo",
      "args": ["hi"]
    },
    "remote": {
      "type": "http",
      "url": "https://example.com/mcp",
      "enabled": true
    },
    "off": {
      "command": "false",
      "enabled": false
    }
  }
}"#,
        )
        .unwrap();

        let profiles = crate::acp::profile::ProfileMiddleware::from_config(
            crate::acp::ProfileConfig::builtin_defaults(),
        );
        // Default caps: stdio always ok; http/sse off unless advertised.
        // Omitted mcpServers = allow all capability-eligible servers.
        let stdio_only = super::load_session_mcp_servers(
            Some(&path),
            &McpCapabilities::new(),
            &profiles,
            "sess-test",
        )
        .await;
        assert_eq!(stdio_only.len(), 1);
        assert!(matches!(stdio_only[0], McpServer::Stdio(_)));

        let with_http = super::load_session_mcp_servers(
            Some(&path),
            &McpCapabilities::new().http(true),
            &profiles,
            "sess-test",
        )
        .await;
        assert_eq!(with_http.len(), 2);

        // Malformed config must not fail session create.
        fs::write(&path, "{not-json").unwrap();
        assert!(super::load_session_mcp_servers(
            Some(&path),
            &McpCapabilities::new(),
            &profiles,
            "sess-test",
        )
        .await
        .is_empty());
        assert!(super::load_session_mcp_servers(
            None,
            &McpCapabilities::new(),
            &profiles,
            "sess-test",
        )
        .await
        .is_empty());
    }

    #[tokio::test]
    async fn load_session_mcp_servers_respects_profile_server_allowlist() {
        use agent_client_protocol::schema::v1::{McpCapabilities, McpServer};
        use std::collections::BTreeMap;
        use std::fs;

        use crate::acp::profile_config::{Profile, ProfileConfig};

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("mcp.json");
        fs::write(
            &path,
            r#"{
  "version": 1,
  "mcpServers": {
    "alpha": { "command": "true" },
    "beta": { "command": "true" }
  }
}"#,
        )
        .unwrap();

        let mut profiles_map = BTreeMap::new();
        profiles_map.insert(
            "code".to_string(),
            Profile {
                label: "Code".into(),
                instructions: "x".into(),
                mcp_servers: Some(vec!["beta".into()]),
                legacy_tools: None,
            },
        );
        let profile_cfg = ProfileConfig {
            profiles: profiles_map,
            default_profile_id: "code".into(),
        };
        let profiles = crate::acp::profile::ProfileMiddleware::from_config(profile_cfg);

        let filtered = super::load_session_mcp_servers(
            Some(&path),
            &McpCapabilities::new(),
            &profiles,
            "sess-allowlist",
        )
        .await;
        assert_eq!(filtered.len(), 1);
        assert!(matches!(&filtered[0], McpServer::Stdio(s) if s.name == "beta"));

        // Missing mcpServers allows all capability-eligible configured servers.
        let open = crate::acp::profile::ProfileMiddleware::from_config(
            crate::acp::ProfileConfig::builtin_defaults(),
        );
        let all = super::load_session_mcp_servers(
            Some(&path),
            &McpCapabilities::new(),
            &open,
            "sess-all",
        )
        .await;
        assert_eq!(all.len(), 2);

        let none = crate::acp::profile::ProfileMiddleware::from_config(ProfileConfig {
            profiles: BTreeMap::from([(
                "code".to_string(),
                Profile {
                    label: "Code".into(),
                    instructions: "x".into(),
                    mcp_servers: Some(Vec::new()),
                    legacy_tools: None,
                },
            )]),
            default_profile_id: "code".into(),
        });
        assert!(super::load_session_mcp_servers(
            Some(&path),
            &McpCapabilities::new(),
            &none,
            "sess-none",
        )
        .await
        .is_empty());
    }
}
