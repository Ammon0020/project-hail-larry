//! Profile config REST handlers (`GET`/`PUT /api/profiles`).
//!
//! Auth is enforced by the shared protected-router middleware (same as MCP).
//! Validation reuses [`ProfileConfig::parse`] / [`ProfileConfig::validate`] so
//! REST and on-disk loader rules cannot drift. Body size is capped at
//! [`crate::acp::MAX_FILE_BYTES`] (256 KiB), matching the file-size limit.
//!
//! # Trust model
//!
//! Caller is any paired device (or loopback). Worst case is rewriting local
//! profile instructions / tool whitelist — no shell or path execution here.

use axum::body::Bytes;
use axum::extract::State;
use axum::Json;
use tracing::{error, info};

use crate::acp::{ProfileConfig, ProfileConfigError, MAX_FILE_BYTES};
use crate::mcp::File as McpFile;

use super::{ApiResponseError, AppState};

/// `GET /api/profiles` — resolved in-memory config (built-ins when no file).
pub async fn get_profiles(
    State(state): State<AppState>,
) -> Result<Json<ProfileConfig>, ApiResponseError> {
    let config = state.acp.profile_config().map_err(|error| {
        error!(%error, "read profile config failed");
        ApiResponseError::internal(format!("read profile config: {error}"))
    })?;
    Ok(Json(config))
}

/// `PUT /api/profiles` — validate, atomic write, then replace in-memory config.
///
/// Rejects oversized bodies with 413 before JSON parse. Validation failures
/// return 400 and leave the existing file untouched.
pub async fn put_profiles(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Json<ProfileConfig>, ApiResponseError> {
    // Cap before parse so a huge payload cannot burn CPU on serde.
    if body.len() as u64 > MAX_FILE_BYTES {
        return Err(ApiResponseError::payload_too_large(format!(
            "profile config body too large: {} bytes (max {MAX_FILE_BYTES})",
            body.len()
        )));
    }

    let config = ProfileConfig::parse(&body).map_err(|e| profile_config_error(&e))?;
    validate_profile_mcp_servers(&config, state.mcp_config_path.as_deref())?;

    // Persist first; only swap memory after a durable write succeeds.
    config.save().map_err(|e| profile_config_error(&e))?;

    state
        .acp
        .replace_profile_config(config.clone())
        .map_err(|error| {
            // Disk is already updated; surface loudly so operators reload/restart.
            error!(%error, "profiles.json saved but in-memory replace failed");
            ApiResponseError::internal(format!(
                "profiles.json saved but failed to update running config: {error}"
            ))
        })?;

    info!(
        profiles = config.profiles.len(),
        default = %config.default_profile_id,
        "updated profiles.json"
    );
    Ok(Json(config))
}

/// Maps loader/save validation errors to stable client-facing HTTP statuses.
fn profile_config_error(error: &ProfileConfigError) -> ApiResponseError {
    match error {
        ProfileConfigError::Json(inner) => {
            ApiResponseError::bad_request(format!("invalid profile config JSON: {inner}"))
        }
        ProfileConfigError::FileTooLarge { size, max } => ApiResponseError::payload_too_large(
            format!("profile config too large: {size} bytes (max {max})"),
        ),
        ProfileConfigError::TooManyProfiles { .. }
        | ProfileConfigError::DefaultProfileMissing(_)
        | ProfileConfigError::InvalidProfileId(_)
        | ProfileConfigError::LabelTooLong { .. }
        | ProfileConfigError::InstructionsTooLong { .. }
        | ProfileConfigError::UnsafeToolName { .. }
        | ProfileConfigError::UnsafeMcpServerName { .. }
        | ProfileConfigError::UnknownMcpServer { .. }
        | ProfileConfigError::Validation(_) => ApiResponseError::bad_request(error.to_string()),
        ProfileConfigError::Io(inner) => {
            error!(%inner, "write profiles.json failed");
            ApiResponseError::internal(format!("write profiles.json: {inner}"))
        }
        ProfileConfigError::Config(inner) => {
            error!(%inner, "resolve profiles.json path failed");
            ApiResponseError::internal(format!("resolve profiles.json path: {inner}"))
        }
    }
}

/// Validate explicit server selections against the configured `mcp.json`.
/// Disabled servers remain valid profile choices because they may be enabled
/// later; unavailable names are rejected before replacing the live config.
fn validate_profile_mcp_servers(
    config: &ProfileConfig,
    path: Option<&std::path::Path>,
) -> Result<(), ApiResponseError> {
    let Some(path) = path else {
        return Ok(());
    };
    let file = McpFile::load(path).map_err(|error| {
        error!(%error, "load mcp config while validating profiles failed");
        ApiResponseError::internal(format!("load mcp config: {error}"))
    })?;
    config
        .validate_mcp_servers_against(file.mcp_servers.keys().map(String::as_str))
        .map_err(|e| profile_config_error(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{lock_state_dir_env, STATE_DIR_ENV_VAR};
    use axum::body::Body;
    use axum::extract::ConnectInfo;
    use axum::http::{header, Method, Request, StatusCode};
    use axum::response::Response;
    use serde_json::{json, Value};
    use std::net::SocketAddr;
    use std::sync::MutexGuard;
    use tower::ServiceExt;

    /// Holds `LOCAL_AGENT_STATE_DIR` for the lifetime of the returned guard.
    ///
    /// Must be kept alive across `.await` points so PUT handlers that call
    /// [`ProfileConfig::path`] see the isolated directory. Uses the shared
    /// [`lock_state_dir_env`] so concurrent modules cannot race the env var.
    struct StateDirGuard {
        _lock: MutexGuard<'static, ()>,
        prior: Option<std::ffi::OsString>,
    }

    impl Drop for StateDirGuard {
        fn drop(&mut self) {
            match &self.prior {
                Some(v) => std::env::set_var(STATE_DIR_ENV_VAR, v),
                None => std::env::remove_var(STATE_DIR_ENV_VAR),
            }
        }
    }

    fn pin_state_dir(dir: &std::path::Path) -> StateDirGuard {
        let lock = lock_state_dir_env();
        let prior = std::env::var_os(STATE_DIR_ENV_VAR);
        std::env::set_var(STATE_DIR_ENV_VAR, dir);
        StateDirGuard { _lock: lock, prior }
    }

    fn state_in(dir: &tempfile::TempDir) -> AppState {
        crate::api::test_support::test_state(dir.path())
    }

    async fn oneshot_peer(state: AppState, mut request: Request<Body>, peer: &str) -> Response {
        let addr: SocketAddr = peer.parse().expect("peer");
        request.extensions_mut().insert(ConnectInfo(addr));
        crate::api::router(state)
            .oneshot(request)
            .await
            .expect("response")
    }

    async fn oneshot(state: AppState, request: Request<Body>) -> Response {
        oneshot_peer(state, request, "127.0.0.1:9").await
    }

    async fn body_json(response: Response) -> Value {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        serde_json::from_slice(&bytes).expect("json")
    }

    fn valid_custom_config() -> Value {
        json!({
            "profiles": {
                "code": {
                    "label": "Code",
                    "instructions": "code-instr",
                    "mcpServers": ["context7"]
                },
                "review": {
                    "label": "Review",
                    "instructions": "review-only",
                    "mcpServers": []
                }
            },
            "defaultProfileId": "review"
        })
    }

    #[tokio::test]
    async fn get_returns_builtin_defaults_when_no_file() {
        let dir = tempfile::tempdir().expect("temp");
        let _pin = pin_state_dir(dir.path());
        let state = state_in(&dir);
        let response = oneshot(
            state,
            Request::builder()
                .uri("/api/profiles")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        let body = body_json(response).await;
        assert_eq!(body["defaultProfileId"], "code");
        assert!(body["profiles"]["code"].is_object());
        assert!(body["profiles"]["ask"].is_object());
        assert!(body["profiles"]["plan"].is_object());
        assert_eq!(body["profiles"]["code"]["label"], "Code");
    }

    #[tokio::test]
    async fn put_then_get_round_trip_and_updates_middleware() {
        let dir = tempfile::tempdir().expect("temp");
        let _pin = pin_state_dir(dir.path());
        let path = dir.path().join("profiles.json");
        let state = state_in(&dir);

        let put = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri("/api/profiles")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(valid_custom_config().to_string()))
                .expect("request"),
        )
        .await;
        assert_eq!(put.status(), StatusCode::OK);
        let put_body = body_json(put).await;
        assert_eq!(put_body["defaultProfileId"], "review");

        assert!(path.is_file(), "profiles.json should exist after PUT");
        let on_disk = ProfileConfig::load(&path).expect("load disk");
        assert_eq!(on_disk.default_profile_id, "review");
        assert_eq!(on_disk.profile("review").instructions, "review-only");

        let snap = state.acp.profile_config().expect("snapshot");
        assert_eq!(snap.default_profile_id, "review");
        assert_eq!(snap.profile("review").instructions, "review-only");

        let get = oneshot(
            state,
            Request::builder()
                .uri("/api/profiles")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(get.status(), StatusCode::OK);
        let get_body = body_json(get).await;
        assert_eq!(get_body["defaultProfileId"], "review");
        assert_eq!(get_body["profiles"]["review"]["mcpServers"], json!([]));
    }

    #[tokio::test]
    async fn put_invalid_leaves_existing_file_untouched() {
        let dir = tempfile::tempdir().expect("temp");
        let _pin = pin_state_dir(dir.path());
        let path = dir.path().join("profiles.json");
        let good = ProfileConfig::builtin_defaults();
        good.save_to(&path).expect("seed");
        let before = std::fs::read(&path).expect("read before");

        let state = state_in(&dir);
        state.acp.replace_profile_config(good).expect("seed memory");

        let cases: Vec<(&str, Bytes)> = vec![
            (
                "unknown field",
                Bytes::from(
                    r#"{"profiles":{"code":{"label":"C","instructions":"i","tools":[]}},"defaultProfileId":"code","extra":1}"#,
                ),
            ),
            (
                "bad tool name",
                Bytes::from(
                    r#"{"profiles":{"code":{"label":"C","instructions":"i","tools":["rm -rf /"]}},"defaultProfileId":"code"}"#,
                ),
            ),
            (
                "dangling default",
                Bytes::from(
                    r#"{"profiles":{"code":{"label":"C","instructions":"i","tools":[]}},"defaultProfileId":"nope"}"#,
                ),
            ),
            (
                "invalid profile id",
                Bytes::from(
                    r#"{"profiles":{"bad id":{"label":"C","instructions":"i","tools":[]}},"defaultProfileId":"bad id"}"#,
                ),
            ),
            (
                "instructions too long",
                Bytes::from(
                    json!({
                        "profiles": {
                            "code": {
                                "label": "C",
                                "instructions": "x".repeat(16 * 1024 + 1),
                                "tools": []
                            }
                        },
                        "defaultProfileId": "code"
                    })
                    .to_string(),
                ),
            ),
            ("not json", Bytes::from("{not json")),
        ];

        for (label, body) in cases {
            let response = oneshot(
                state.clone(),
                Request::builder()
                    .method(Method::PUT)
                    .uri("/api/profiles")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .expect("request"),
            )
            .await;
            assert_eq!(
                response.status(),
                StatusCode::BAD_REQUEST,
                "case {label} should be 400"
            );
            let after = std::fs::read(&path).expect("read after");
            assert_eq!(after, before, "case {label} must not rewrite file");
            let snap = state.acp.profile_config().expect("snap");
            assert_eq!(
                snap.default_profile_id, "code",
                "case {label} must not mutate memory"
            );
        }
    }

    #[tokio::test]
    async fn put_oversized_body_returns_413() {
        let dir = tempfile::tempdir().expect("temp");
        let _pin = pin_state_dir(dir.path());
        let path = dir.path().join("profiles.json");
        let good = ProfileConfig::builtin_defaults();
        good.save_to(&path).expect("seed");
        let before = std::fs::read(&path).expect("before");

        let state = state_in(&dir);
        // MAX_FILE_BYTES is 256 KiB, well within usize on all supported targets.
        #[allow(clippy::cast_possible_truncation)]
        let oversized = vec![b'a'; (MAX_FILE_BYTES as usize) + 1];
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::PUT)
                .uri("/api/profiles")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(oversized))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert_eq!(std::fs::read(&path).expect("after"), before);
    }

    #[tokio::test]
    async fn endpoints_require_auth_off_loopback() {
        let dir = tempfile::tempdir().expect("temp");
        let state = state_in(&dir);

        let get = oneshot_peer(
            state.clone(),
            Request::builder()
                .uri("/api/profiles")
                .body(Body::empty())
                .expect("request"),
            "192.168.1.50:4000",
        )
        .await;
        assert_eq!(get.status(), StatusCode::UNAUTHORIZED);

        let put = oneshot_peer(
            state,
            Request::builder()
                .method(Method::PUT)
                .uri("/api/profiles")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{}"))
                .expect("request"),
            "192.168.1.50:4000",
        )
        .await;
        assert_eq!(put.status(), StatusCode::UNAUTHORIZED);
    }
}
