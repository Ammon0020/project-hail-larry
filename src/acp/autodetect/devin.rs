//! Devin harness discovery.
//!
//! Devin ACP intentionally ignores local CLI credentials. Models are listed via
//! `session/new` → `configOptions` after ACP `authenticate`:
//! 1. Prefer non-interactive `_meta.api_key` from the local credentials file
//!    (already present after the user's first host-side login).
//! 2. Fall back to browser PKCE (`methodId: devin-browser`) with a hard timeout
//!    so a skipped login cannot hang daemon startup.
//!
//! No hardcoded model catalog.

use std::path::{Path, PathBuf};
use std::pin::Pin;

use serde_json::{json, Value};
use tokio::io::BufReader;

use crate::config::AgentModel;

use super::common::{models_from_session_config, wait_for_response, with_acp_child, write_json};
use super::{AutodetectOptions, Harness};

pub(super) struct Devin;

impl Harness for Devin {
    fn id(&self) -> &'static str {
        "devin"
    }

    fn name(&self) -> &'static str {
        "Devin"
    }

    fn commands(&self) -> &'static [&'static str] {
        &["devin"]
    }

    fn args(&self) -> &'static [&'static str] {
        &["acp"]
    }

    fn search_paths(&self) -> &'static [&'static str] {
        &[
            "%LOCALAPPDATA%\\Programs\\Devin\\resources\\app\\extensions\\windsurf\\devin\\bin",
            "/Applications/Devin.app/Contents/Resources/app/extensions/windsurf/devin/bin",
            "~/.local/share/Devin/resources/app/extensions/windsurf/devin/bin",
            "%LOCALAPPDATA%\\Programs\\Windsurf\\resources\\app\\extensions\\windsurf\\devin\\bin",
            "/Applications/Windsurf.app/Contents/Resources/app/extensions/windsurf/devin/bin",
        ]
    }

    fn detect_models(
        &self,
        command: &Path,
        options: AutodetectOptions,
    ) -> Pin<Box<dyn std::future::Future<Output = (Vec<AgentModel>, String)> + Send + '_>> {
        let command = command.to_path_buf();
        Box::pin(async move {
            match models_from_acp_session(&command, options).await {
                Ok(models) if !models.is_empty() => (models, String::new()),
                Ok(_) => (
                    Vec::new(),
                    "Devin models unavailable (authenticate on the host)".into(),
                ),
                Err(error) => {
                    tracing::warn!(error = %error, "Devin model probe failed");
                    (Vec::new(), format!("Devin model probe failed: {error}"))
                }
            }
        })
    }
}

async fn models_from_acp_session(
    command: &Path,
    options: AutodetectOptions,
) -> Result<Vec<AgentModel>, String> {
    let api_key = read_local_api_key();
    // API-key path is non-interactive — use the short probe timeout.
    // Browser PKCE may open a browser; use auth_probe_timeout and kill on expiry.
    let (duration, use_browser) = if api_key.is_some() {
        (options.probe_timeout, false)
    } else {
        (options.auth_probe_timeout, true)
    };

    with_acp_child(command, &["acp"], duration, move |mut stdin, stdout| {
        async move {
            write_json(
                &mut stdin,
                json!({
                    "jsonrpc": "2.0",
                    "id": 1,
                    "method": "initialize",
                    "params": {
                        "protocolVersion": 1,
                        "clientInfo": {"name": "local-agent-autodetect", "version": "1.0"},
                        "clientCapabilities": {}
                    }
                }),
            )
            .await?;
            let mut stdout = BufReader::new(stdout);
            let init = wait_for_response(&mut stdout, 1).await?;
            let method_id = pick_auth_method(&init).unwrap_or("devin-browser");

            let mut auth_params = json!({"methodId": method_id});
            if let Some(key) = api_key {
                // Devin accepts credentials via `_meta.api_key` (non-interactive).
                auth_params["_meta"] = json!({"api_key": key});
            } else if use_browser {
                tracing::info!(
                    "Devin ACP auth: starting browser PKCE (timeout {}s)",
                    duration.as_secs()
                );
            }

            write_json(
                &mut stdin,
                json!({
                    "jsonrpc": "2.0",
                    "id": 2,
                    "method": "authenticate",
                    "params": auth_params
                }),
            )
            .await?;
            wait_for_response(&mut stdout, 2).await?;

            let cwd = std::env::temp_dir().to_string_lossy().into_owned();
            write_json(
                &mut stdin,
                json!({
                    "jsonrpc": "2.0",
                    "id": 3,
                    "method": "session/new",
                    "params": {"cwd": cwd, "mcpServers": []}
                }),
            )
            .await?;
            let session = wait_for_response(&mut stdout, 3).await?;
            let models = session
                .get("result")
                .map(models_from_session_config)
                .unwrap_or_default();
            Ok(models)
        }
    })
    .await
}

fn pick_auth_method(init: &Value) -> Option<&str> {
    let methods = init
        .get("result")
        .and_then(|r| r.get("authMethods"))
        .and_then(Value::as_array)?;
    // Prefer a browser/login method when present.
    for method in methods {
        let id = method.get("id").and_then(Value::as_str)?;
        if id.contains("browser") || id.contains("login") {
            return Some(id);
        }
    }
    methods
        .first()
        .and_then(|m| m.get("id"))
        .and_then(Value::as_str)
}

/// Read a host-local Devin API key for non-interactive ACP authenticate.
///
/// Prefer env override for tests/CI, then the standard credentials.toml path.
fn read_local_api_key() -> Option<String> {
    if let Ok(key) = std::env::var("DEVIN_API_KEY") {
        if !key.is_empty() {
            return Some(key);
        }
    }
    let path = credentials_path()?;
    let contents = std::fs::read_to_string(path).ok()?;
    parse_api_key_from_credentials(&contents)
}

fn credentials_path() -> Option<PathBuf> {
    let home = dirs::home_dir()?;
    let path = home.join(".local/share/devin/credentials.toml");
    path.is_file().then_some(path)
}

fn parse_api_key_from_credentials(contents: &str) -> Option<String> {
    // Minimal TOML key scrape — avoid pulling the whole file into a struct so
    // we never log or re-serialize secrets. Prefer windsurf_api_key, then api_key.
    for key_name in ["windsurf_api_key", "api_key"] {
        for line in contents.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else {
                continue;
            };
            if k.trim() != key_name {
                continue;
            }
            let v = v.trim().trim_matches('"').trim_matches('\'');
            if !v.is_empty() {
                return Some(v.to_string());
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::{parse_api_key_from_credentials, pick_auth_method};
    use crate::acp::autodetect::common::models_from_session_config;
    use serde_json::json;

    #[test]
    fn credentials_parser_reads_windsurf_key() {
        let key = parse_api_key_from_credentials(
            "windsurf_api_key = \"secret-value\"\napi_server_url = \"https://example\"\n",
        );
        assert_eq!(key.as_deref(), Some("secret-value"));
    }

    #[test]
    fn credentials_parser_ignores_empty() {
        assert!(parse_api_key_from_credentials("windsurf_api_key = \"\"\n").is_none());
        assert!(parse_api_key_from_credentials("# comment\n").is_none());
    }

    #[test]
    fn pick_auth_prefers_browser_method() {
        let init = json!({
            "result": {
                "authMethods": [
                    {"id": "other"},
                    {"id": "devin-browser", "name": "Log in with browser"}
                ]
            }
        });
        assert_eq!(pick_auth_method(&init), Some("devin-browser"));
    }

    #[test]
    fn session_models_shape_is_generic() {
        // Fixture mirrors Devin session/new shape without pinning live model ids.
        let result = json!({
            "configOptions": [{
                "id": "model",
                "category": "model",
                "options": [
                    {"value": "alpha", "name": "Alpha"},
                    {"value": "beta", "name": "Beta"},
                    {"value": "gamma", "name": "Gamma"}
                ]
            }]
        });
        let models = models_from_session_config(&result);
        assert!(models.len() >= 2);
        let ids: std::collections::HashSet<_> = models.iter().map(|m| m.id.as_str()).collect();
        assert_eq!(ids.len(), models.len());
        for m in &models {
            assert!(!m.id.is_empty());
            assert!(!m.name.is_empty());
        }
    }
}
