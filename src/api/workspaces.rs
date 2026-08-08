//! Workspace registration, removal, trust, and pending-registration handlers.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::warn;

use crate::interfaces::{Event, EventType, WorkspaceManager};
use crate::sync::is_loopback_addr;
use crate::workspace::tabs::WorkspaceTabs;

use super::auth::{device_id_from_request, PeerAddr};
use super::{
    app_error, cancel_pending_action, decode_json_body, pairing_error, record_event,
    revocation_grace_period, ApiResponseError, AppState, CancelActionRequest,
};

pub(super) async fn list_workspaces(
    State(state): State<AppState>,
) -> Result<Json<Vec<crate::interfaces::WorkspaceInfo>>, ApiResponseError> {
    let mut workspaces = state.workspaces.list().await.map_err(app_error)?;
    // Join preview trust state from config so the frontend knows whether to
    // prompt (None), allow (Some(true)), or restrict (Some(false)).
    let trust = state.config.read().clone();
    for workspace in &mut workspaces {
        workspace.trusted = trust.workspace_trust(&workspace.id);
    }
    Ok(Json(workspaces))
}

#[derive(Deserialize)]
pub(super) struct RegisterWorkspaceRequest {
    path: String,
}

/// `POST /api/workspaces` — gated by `allowRemoteWorkspaceRegistration`.
/// Loopback does NOT bypass this policy (Go parity): host registration stays on
/// `app add-folder` writing config; the HTTP surface stays closed when remote
/// registration is disabled — including from 127.0.0.1 (contract harness).
pub(super) async fn register_workspace(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    headers: HeaderMap,
    body: Result<Json<RegisterWorkspaceRequest>, JsonRejection>,
) -> Result<Response, ApiResponseError> {
    let Json(payload) = decode_json_body(body)?;
    let loopback = is_loopback_addr(&remote_addr);
    let allow_remote = state.config.read().allow_remote_workspace_registration;
    // No loopback exception: Go returns 403 unconditionally when the flag is
    // false; CLI add-folder persists config rather than calling this endpoint.
    if !allow_remote {
        return Err(ApiResponseError::forbidden(
            "Remote workspace registration is disabled. Use 'app add-folder <path>' on the host, or set allowRemoteWorkspaceRegistration: true in config.",
        ));
    }

    let grace = revocation_grace_period(&state);
    // Host CLI and zero-grace paths register immediately into the live manager.
    if loopback || grace.is_zero() {
        let workspace = state
            .workspaces
            .register(&payload.path)
            .await
            .map_err(app_error)?;
        if let Some(watcher) = state.fs_watcher.as_ref() {
            watcher.add_workspace(&workspace.id, &workspace.path);
        }
        // Keep config.toml aligned when the host CLI (or zero-grace remote)
        // registers into a running daemon.
        if let Err(error) = state.config.write().add_workspace(&workspace.path) {
            warn!(
                path = %workspace.path,
                %error,
                "registered workspace but failed to persist config"
            );
        }
        return Ok((StatusCode::CREATED, Json(workspace)).into_response());
    }

    let requester = device_id_from_request(&headers);
    let info = state
        .pairing
        .request_workspace_registration(&payload.path, requester, grace)
        .map_err(pairing_error)?;

    let mut event = Event::new(0, EventType::WorkspaceRegistrationPending, "", Utc::now());
    event.target.clone_from(&info.path);
    event.request_id.clone_from(&info.id);
    event.command.clone_from(&info.requested_by);
    event.execute_at = info.execute_at;
    record_event(&state, event).await;

    Ok((StatusCode::ACCEPTED, Json(info)).into_response())
}

/// `DELETE /api/workspaces/{id}` — host/loopback unregistration for
/// `remove-folder` against a running daemon. LAN clients stay on the remote
/// registration gate and cannot delete workspaces here.
pub(super) async fn remove_workspace(
    State(state): State<AppState>,
    PeerAddr(remote_addr): PeerAddr,
    Path(id): Path<String>,
) -> Result<Json<Value>, ApiResponseError> {
    if !is_loopback_addr(&remote_addr) {
        return Err(ApiResponseError::forbidden(
            "Workspace removal from the network is disabled. Use 'app remove-folder <id>' on the host.",
        ));
    }
    let workspaces = state.workspaces.list().await.map_err(app_error)?;
    let workspace = workspaces
        .into_iter()
        .find(|workspace| workspace.id == id)
        .ok_or_else(|| ApiResponseError::not_found(format!("workspace {id} not found")))?;
    state.workspaces.remove(&id).await.map_err(app_error)?;
    if let Some(watcher) = state.fs_watcher.as_ref() {
        watcher.remove_workspace(&id);
    }
    // Drop the workspace's stored tabs so entries in workspace-tabs.json do not
    // outlive the workspace. Treated as non-fatal: the workspace is already
    // gone from the live manager, so failing the request would be worse than
    // an orphaned entry — matches the config-drift warning below.
    if let Err(error) = state.tabs.remove(&id) {
        warn!(
            workspace_id = %id,
            %error,
            "removed live workspace but tab cleanup failed"
        );
    }
    if let Err(error) = state.config.write().remove_workspace(&workspace.path) {
        // Already removed from the live manager; config drift is loud but not
        // fatal for the CLI operator who may have already saved config.
        warn!(
            path = %workspace.path,
            %error,
            "removed live workspace but config persist failed"
        );
    }
    Ok(Json(json!({ "status": "removed", "id": id })))
}

#[derive(Deserialize)]
pub(super) struct SetWorkspaceTrustRequest {
    trusted: Option<bool>,
}

/// `PUT /api/workspaces/{id}/trust` — sets the preview trust state for a
/// workspace. Body `{"trusted": true}` → trusted (permissive CSP),
/// `{"trusted": false}` → untrusted (restrictive CSP), `{"trusted": null}`
/// → reset to unknown (prompt on next HTML preview). Any paired device can
/// set this — it's a UI preference, not a security boundary (the CSP
/// enforcement is server-side and authoritative).
pub(super) async fn set_workspace_trust(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<SetWorkspaceTrustRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(payload) = decode_json_body(body)?;
    // Verify the workspace exists before persisting trust for a phantom ID.
    let workspaces = state.workspaces.list().await.map_err(app_error)?;
    if !workspaces.iter().any(|workspace| workspace.id == id) {
        return Err(ApiResponseError::not_found(format!(
            "workspace {id} not found"
        )));
    }
    state
        .config
        .write()
        .set_workspace_trust(&id, payload.trusted)
        .map_err(|error| ApiResponseError::internal(format!("persist workspace trust: {error}")))?;
    Ok(Json(json!({ "id": id, "trusted": payload.trusted })))
}

/// `GET /api/workspaces/{id}/tabs` — restorable editor tabs for a workspace.
///
/// Returns an empty set for a workspace whose tabs have never been saved,
/// which is the normal first-open state rather than an error.
pub(super) async fn get_workspace_tabs(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<WorkspaceTabs>, ApiResponseError> {
    require_workspace(&state, &id).await?;
    state.tabs.load(&id).map(Json).map_err(app_error)
}

/// `PUT /api/workspaces/{id}/tabs` — replace a workspace's tab set.
///
/// Any paired device may write this: it is editor layout, not a security
/// boundary. The payload is capped (tab count and field lengths) because a
/// client could otherwise grow the daemon's state file without bound.
/// File content is deliberately not accepted — unsaved buffers stay on the
/// device that typed them.
pub(super) async fn put_workspace_tabs(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<WorkspaceTabs>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    require_workspace(&state, &id).await?;
    let Json(payload) = decode_json_body(body)?;
    let validated = payload.validated().map_err(app_error)?;
    let count = validated.tabs.len();
    state.tabs.save(&id, validated).map_err(app_error)?;
    Ok(Json(json!({ "id": id, "tabs": count })))
}

/// 404 unless `id` names a registered workspace, so tabs cannot be stored
/// against a phantom id and accumulate forever.
async fn require_workspace(state: &AppState, id: &str) -> Result<(), ApiResponseError> {
    let workspaces = state.workspaces.list().await.map_err(app_error)?;
    if workspaces.iter().any(|workspace| workspace.id == id) {
        return Ok(());
    }
    Err(ApiResponseError::not_found(format!(
        "workspace {id} not found"
    )))
}

/// `POST /api/workspaces/cancel-registration` — body `{"actionId":"..."}`.
pub(super) async fn cancel_workspace_registration(
    State(state): State<AppState>,
    body: Result<Json<CancelActionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    cancel_pending_action(
        &state,
        decode_json_body(body)?.0,
        |id| state.pairing.cancel_workspace_registration(id),
        EventType::WorkspaceRegistrationCancelled,
    )
    .await
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{header, Method, Request, StatusCode};

    use crate::api::test_support::{
        bearer, json_body, oneshot, oneshot_peer, pending_actions_state, state, StateDirEnvGuard,
    };
    use crate::interfaces::WorkspaceManager;

    #[tokio::test]
    async fn workspace_registration_disabled_returns_403() {
        let (_dir, state, credential) = pending_actions_state(300, false);
        let response = oneshot_peer(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, bearer(&credential))
                .body(Body::from(r#"{"path":"/some/path"}"#))
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        let body = json_body(response).await;
        assert!(
            body["error"]
                .as_str()
                .unwrap_or_default()
                .contains("Remote workspace registration is disabled"),
            "body: {body}"
        );
    }

    #[tokio::test]
    async fn workspace_registration_enabled_returns_accepted() {
        let (_dir, state, credential) = pending_actions_state(300, true);
        let response = oneshot_peer(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, bearer(&credential))
                .body(Body::from(r#"{"path":"/some/path"}"#))
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(response.status(), StatusCode::ACCEPTED);
        let info = json_body(response).await;
        assert_eq!(info["type"], "workspace_registration");
        assert_eq!(info["path"], "/some/path");
    }

    #[tokio::test]
    async fn cancel_workspace_registration_removes_pending_action() {
        let (_dir, state, credential) = pending_actions_state(300, true);
        let registration = oneshot_peer(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces")
                .header(header::CONTENT_TYPE, "application/json")
                .header(header::AUTHORIZATION, bearer(&credential))
                .body(Body::from(r#"{"path":"/some/path"}"#))
                .expect("request"),
            "10.0.0.1:9",
        )
        .await;
        assert_eq!(registration.status(), StatusCode::ACCEPTED);
        let info = json_body(registration).await;
        let action_id = info["id"].as_str().expect("action id").to_owned();

        let cancellation = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces/cancel-registration")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"actionId":"{action_id}"}}"#)))
                .expect("request"),
        )
        .await;
        assert_eq!(cancellation.status(), StatusCode::OK);

        let list = oneshot(
            state,
            Request::builder()
                .uri("/api/pending-actions")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        let pending = json_body(list).await;
        assert_eq!(pending.as_array().map_or(1, Vec::len), 0);
    }

    #[tokio::test]
    async fn cancel_workspace_registration_bad_body_returns_400() {
        let (_dir, state, _credential) = pending_actions_state(300, true);
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::POST)
                .uri("/api/workspaces/cancel-registration")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from("{not json"))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        assert_eq!(json_body(response).await["error"], "invalid request body");
    }

    /// Trust state cannot be persisted for a workspace that does not exist.
    #[tokio::test]
    async fn set_workspace_trust_returns_404_for_unknown_workspace() {
        let (_dir, state) = state();
        let response = oneshot(
            state,
            Request::builder()
                .method(Method::PUT)
                .uri("/api/workspaces/nonexistent-id/trust")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"trusted":true}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    /// Trust updates are persisted and joined back into workspace list results.
    /// Tabs must not be storable against an unregistered id — otherwise a
    /// client could accumulate entries for workspaces that never existed.
    #[tokio::test]
    async fn workspace_tabs_reject_an_unknown_workspace() {
        let (_dir, state) = state();
        for (method, body) in [
            (Method::GET, Body::empty()),
            (Method::PUT, Body::from(r#"{"tabs":[]}"#)),
        ] {
            let response = oneshot(
                state.clone(),
                Request::builder()
                    .method(method)
                    .uri("/api/workspaces/ws-nope/tabs")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body)
                    .expect("request"),
            )
            .await;
            assert_eq!(response.status(), StatusCode::NOT_FOUND);
        }
    }

    /// The payload cap is the only bound on how large a paired device can make
    /// the daemon's tab file.
    #[tokio::test]
    async fn workspace_tabs_reject_an_oversized_payload() {
        let (state_dir, state) = state();
        let _env = StateDirEnvGuard::pin(state_dir.path());
        let workspace_dir = tempfile::tempdir().expect("workspace directory");
        let workspace = state
            .workspaces
            .register(workspace_dir.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");

        let tabs: Vec<String> = (0..=crate::workspace::tabs::MAX_TABS_PER_WORKSPACE)
            .map(|i| format!(r#"{{"id":"t{i}","path":"a","name":"a"}}"#))
            .collect();
        let response = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(r#"{{"tabs":[{}]}}"#, tabs.join(","))))
                .expect("request"),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    /// A saved layout must come back on the next open — the whole feature.
    #[tokio::test]
    async fn workspace_tabs_round_trip_for_a_registered_workspace() {
        let (state_dir, state) = state();
        let _env = StateDirEnvGuard::pin(state_dir.path());
        let workspace_dir = tempfile::tempdir().expect("workspace directory");
        let workspace = state
            .workspaces
            .register(workspace_dir.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");

        let saved = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"tabs":[{"id":"t1","path":"src/a.rs","name":"a.rs"}],"activeTabId":"t1"}"#,
                ))
                .expect("request"),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);

        let loaded = oneshot(
            state.clone(),
            Request::builder()
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(loaded.status(), StatusCode::OK);
        let body = json_body(loaded).await;
        assert_eq!(body["tabs"][0]["path"], "src/a.rs");
        assert_eq!(body["activeTabId"], "t1");
    }

    #[tokio::test]
    async fn set_workspace_trust_persists_and_reflects_in_list() {
        let (state_dir, state) = state();
        let _env = StateDirEnvGuard::pin(state_dir.path());
        let workspace_dir = tempfile::tempdir().expect("workspace directory");
        let workspace = state
            .workspaces
            .register(workspace_dir.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");

        let list = oneshot(
            state.clone(),
            Request::builder()
                .uri("/api/workspaces")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(list.status(), StatusCode::OK);
        let entries = json_body(list).await;
        let entry = entries
            .as_array()
            .expect("workspaces list")
            .iter()
            .find(|entry| entry["id"] == workspace.id)
            .expect("workspace in list");
        assert!(
            entry.get("trusted").is_none() || entry["trusted"].is_null(),
            "untrusted workspace should have no trusted field, got: {entry}"
        );

        let update = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/trust", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"trusted":true}"#))
                .expect("request"),
        )
        .await;
        assert_eq!(update.status(), StatusCode::OK);
        assert_eq!(json_body(update).await["trusted"], true);

        let list = oneshot(
            state,
            Request::builder()
                .uri("/api/workspaces")
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        let entries = json_body(list).await;
        let entry = entries
            .as_array()
            .expect("workspaces list")
            .iter()
            .find(|entry| entry["id"] == workspace.id)
            .expect("workspace in list");
        assert_eq!(
            entry["trusted"], true,
            "trusted workspace should list trusted=true, got: {entry}"
        );
    }

    /// Removing a workspace must also drop its stored tabs, so entries in
    /// workspace-tabs.json do not outlive the workspace. DELETE is
    /// loopback-only, so the request goes through `oneshot` (127.0.0.1:9).
    #[tokio::test]
    async fn removing_a_workspace_drops_its_stored_tabs() {
        let (state_dir, state) = state();
        let _env = StateDirEnvGuard::pin(state_dir.path());
        let workspace_dir = tempfile::tempdir().expect("workspace directory");
        let workspace = state
            .workspaces
            .register(workspace_dir.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");

        let saved = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"tabs":[{"id":"t1","path":"src/a.rs","name":"a.rs"}],"activeTabId":"t1"}"#,
                ))
                .expect("request"),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);

        let removed = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/workspaces/{}", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(removed.status(), StatusCode::OK);
        assert_eq!(json_body(removed).await["status"], "removed");

        // The workspace is gone, so GET tabs 404s — and the underlying store
        // must have no entry left to resurrect on a future re-registration.
        let loaded = oneshot(
            state.clone(),
            Request::builder()
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(loaded.status(), StatusCode::NOT_FOUND);
        assert!(
            state
                .tabs
                .load(&workspace.id)
                .expect("load tabs")
                .tabs
                .is_empty(),
            "tab store should have no entry for the removed workspace"
        );
    }

    /// `remove_workspace` treats tab cleanup as non-fatal: a `warn!` is logged
    /// and the request still succeeds when `TabStore::remove` fails. We force
    /// that failure by making the state dir read-only so the atomic write
    /// (`O_TMPFILE` + rename) inside `fsutil::atomic_write` is rejected with
    /// `EACCES`. The DELETE must still return 200 with `{"status":"removed"}`
    /// and the workspace must be gone from the live manager.
    ///
    /// Unix-only: the fault relies on filesystem DAC permissions. Skipped when
    /// running as root, since root bypasses those checks and the read-only dir
    /// would not produce a write failure.
    #[cfg(unix)]
    #[tokio::test]
    async fn removing_a_workspace_succeeds_when_tab_cleanup_fails() {
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        /// RAII guard that restores writable permissions on the state dir so
        /// the owning `TempDir` can clean up after the test.
        struct PermGuard(PathBuf);
        impl Drop for PermGuard {
            fn drop(&mut self) {
                let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o700));
            }
        }

        // Root bypasses DAC permissions, so the fault would not fire. Bail
        // before mutating any state rather than silently passing.
        if unsafe { libc::geteuid() } == 0 {
            return;
        }

        let (state_dir, state) = state();
        let _env = StateDirEnvGuard::pin(state_dir.path());
        let workspace_dir = tempfile::tempdir().expect("workspace directory");
        let workspace = state
            .workspaces
            .register(workspace_dir.path().to_str().expect("UTF-8 path"))
            .await
            .expect("register workspace");

        // Persist tabs so the store has a file to rewrite on cleanup.
        let saved = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::PUT)
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    r#"{"tabs":[{"id":"t1","path":"src/a.rs","name":"a.rs"}],"activeTabId":"t1"}"#,
                ))
                .expect("request"),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);

        // Make the state dir read-only (r-x) so the atomic write inside
        // `TabStore::save` cannot create its temp file. Restore writable
        // permissions on drop so the TempDir cleanup succeeds.
        let state_dir_path = state_dir.path().to_path_buf();
        std::fs::set_permissions(&state_dir_path, std::fs::Permissions::from_mode(0o500))
            .expect("set state dir read-only");
        let _perm_guard = PermGuard(state_dir_path);

        // Even though tab cleanup fails, the workspace is already removed from
        // the live manager, so the request must succeed.
        let removed = oneshot(
            state.clone(),
            Request::builder()
                .method(Method::DELETE)
                .uri(format!("/api/workspaces/{}", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(removed.status(), StatusCode::OK);
        assert_eq!(json_body(removed).await["status"], "removed");

        // The workspace is gone from the live manager: GET tabs 404s.
        let loaded = oneshot(
            state.clone(),
            Request::builder()
                .uri(format!("/api/workspaces/{}/tabs", workspace.id))
                .body(Body::empty())
                .expect("request"),
        )
        .await;
        assert_eq!(loaded.status(), StatusCode::NOT_FOUND);
    }
}
