//! Persistent, host-wide settings that do not belong to an agent profile.

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use tracing::error;

use crate::config::PromptContextSettings;

use super::{decode_json_body, ApiResponseError, AppState};

/// Read-only projection of server/network/pairing/security config fields
/// exposed by `GET /api/settings/server` for the frontend settings panel.
///
/// These settings require a daemon restart to change (edit `config.toml` and
/// restart), so the endpoint is read-only — the frontend displays them with
/// "edit config.toml and restart" guidance rather than mutating them.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ServerSettings {
    port: i64,
    host: String,
    tls_enabled: bool,
    https_port: i64,
    tls_cert_dir: String,
    pairing_ttl_seconds: i64,
    credential_inactivity_ttl_seconds: i64,
    allow_remote_workspace_registration: bool,
    revocation_grace_period_seconds: i64,
}

/// `GET /api/settings/prompt-context` — current bounded prompt-path settings.
pub async fn get_prompt_context(State(state): State<AppState>) -> Json<PromptContextSettings> {
    Json(state.config.read().prompt_context.clone())
}

/// `GET /api/settings/server` — read-only server/network/pairing/security
/// config fields for display in the frontend settings panel.
///
/// These settings require a daemon restart to change (edit `config.toml` and
/// restart), so the endpoint is read-only: there is no `PUT`/`PATCH` companion.
pub async fn get_server_settings(State(state): State<AppState>) -> Json<ServerSettings> {
    let config = state.config.read();
    Json(ServerSettings {
        port: config.port,
        host: config.host.clone(),
        tls_enabled: config.tls_enabled,
        https_port: config.https_port,
        tls_cert_dir: config.tls_cert_dir.clone(),
        pairing_ttl_seconds: config.pairing_ttl_seconds,
        credential_inactivity_ttl_seconds: config.credential_inactivity_ttl_seconds,
        allow_remote_workspace_registration: config.allow_remote_workspace_registration,
        revocation_grace_period_seconds: config.revocation_grace_period_seconds,
    })
}

/// `PUT /api/settings/prompt-context` — validate, persist, then update ACP.
///
/// A paired device may reduce or increase bounded path disclosure. The upper
/// bound is enforced before either disk or live state changes.
pub async fn put_prompt_context(
    State(state): State<AppState>,
    body: Result<Json<PromptContextSettings>, JsonRejection>,
) -> Result<Json<PromptContextSettings>, ApiResponseError> {
    let Json(settings) = decode_json_body(body)?;
    settings.validate().map_err(ApiResponseError::bad_request)?;

    {
        let mut config = state.config.write();
        config.prompt_context = settings.clone();
        config.save().map_err(|error| {
            error!(%error, "save prompt context settings failed");
            ApiResponseError::internal(format!("save prompt context settings: {error}"))
        })?;
    }

    state
        .acp
        .replace_prompt_context_settings(settings.clone())
        .map_err(|error| {
            error!(%error, "prompt context settings saved but live update failed");
            ApiResponseError::internal(format!(
                "prompt context settings saved but live update failed: {error}"
            ))
        })?;

    Ok(Json(settings))
}

#[cfg(test)]
mod tests {
    use axum::body::{to_bytes, Body};
    use axum::extract::ConnectInfo;
    use axum::http::{Method, Request, StatusCode};
    use serde_json::Value;
    use std::net::SocketAddr;
    use std::sync::MutexGuard;
    use tower::ServiceExt;

    struct StateDirGuard {
        _lock: MutexGuard<'static, ()>,
        prior: Option<std::ffi::OsString>,
    }

    impl Drop for StateDirGuard {
        fn drop(&mut self) {
            match &self.prior {
                Some(value) => std::env::set_var(crate::config::STATE_DIR_ENV_VAR, value),
                None => std::env::remove_var(crate::config::STATE_DIR_ENV_VAR),
            }
        }
    }

    fn pin_state_dir(dir: &std::path::Path) -> StateDirGuard {
        let lock = crate::config::lock_state_dir_env();
        let prior = std::env::var_os(crate::config::STATE_DIR_ENV_VAR);
        std::env::set_var(crate::config::STATE_DIR_ENV_VAR, dir);
        StateDirGuard { _lock: lock, prior }
    }

    #[tokio::test]
    async fn prompt_context_settings_persist_and_update_live_config() {
        let dir = tempfile::tempdir().expect("temporary state directory");
        let _state_dir = pin_state_dir(dir.path());
        let state = crate::api::test_support::test_state(dir.path());
        let mut request = Request::builder()
            .method(Method::PUT)
            .uri("/api/settings/prompt-context")
            .header("content-type", "application/json")
            .body(Body::from(
                r#"{"openFileLimit":3,"workspaceFileListLimit":4}"#,
            ))
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:9".parse::<SocketAddr>().expect("address"),
        ));

        let response = crate::api::router(state.clone())
            .oneshot(request)
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(state.config.read().prompt_context.open_file_limit, 3);
        assert_eq!(
            state.config.read().prompt_context.workspace_file_list_limit,
            4
        );
        let persisted = std::fs::read_to_string(dir.path().join("config.toml")).expect("config");
        assert!(persisted.contains("openFileLimit = 3"));
        assert!(persisted.contains("workspaceFileListLimit = 4"));
    }

    #[tokio::test]
    async fn get_server_settings_returns_config_values() {
        let dir = tempfile::tempdir().expect("temporary state directory");
        let _state_dir = pin_state_dir(dir.path());
        let state = crate::api::test_support::test_state(dir.path());
        // Seed known server/network/pairing/security values distinct from the
        // defaults so the test would fail if the handler dropped or swapped a
        // field.
        {
            let mut config = state.config.write();
            config.port = 9000;
            config.host = "127.0.0.1".to_string();
            config.tls_enabled = false;
            config.https_port = 9001;
            config.tls_cert_dir = "/custom/tls".to_string();
            config.pairing_ttl_seconds = 120;
            config.credential_inactivity_ttl_seconds = 3600;
            config.allow_remote_workspace_registration = true;
            config.revocation_grace_period_seconds = 60;
        }

        let mut request = Request::builder()
            .method(Method::GET)
            .uri("/api/settings/server")
            .body(Body::empty())
            .expect("request");
        request.extensions_mut().insert(ConnectInfo(
            "127.0.0.1:9".parse::<SocketAddr>().expect("address"),
        ));

        let response = crate::api::router(state.clone())
            .oneshot(request)
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        let body: Value = serde_json::from_slice(&bytes).expect("json body");
        assert_eq!(body["port"], 9000);
        assert_eq!(body["host"], "127.0.0.1");
        assert_eq!(body["tlsEnabled"], false);
        assert_eq!(body["httpsPort"], 9001);
        assert_eq!(body["tlsCertDir"], "/custom/tls");
        assert_eq!(body["pairingTtlSeconds"], 120);
        assert_eq!(body["credentialInactivityTtlSeconds"], 3600);
        assert_eq!(body["allowRemoteWorkspaceRegistration"], true);
        assert_eq!(body["revocationGracePeriodSeconds"], 60);
    }
}
