//! Persistent, host-wide settings that do not belong to an agent profile.

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::Json;
use tracing::error;

use crate::config::PromptContextSettings;

use super::{decode_json_body, ApiResponseError, AppState};

/// `GET /api/settings/prompt-context` — current bounded prompt-path settings.
pub async fn get_prompt_context(State(state): State<AppState>) -> Json<PromptContextSettings> {
    Json(state.config.read().prompt_context.clone())
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
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{Method, Request, StatusCode};
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
}
