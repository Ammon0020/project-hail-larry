//! MCP config and health handlers (Go `internal/server/mcp.go`).
//!
//! GET/PUT preserve raw on-disk JSON formatting. PATCH toggles a single
//! server's `enabled` flag. Status runs on-demand health checks.

use std::path::Path;
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Path as AxumPath, State};
use axum::http::{header, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};
use tracing::error;

use crate::mcp::{self, File as McpFile, McpError, ServerStatus};

use super::{decode_json_body, ApiResponseError, AppState};

#[derive(Deserialize)]
pub(crate) struct PatchMcpServerRequest {
    enabled: Option<bool>,
}

/// `GET /api/mcp` — raw mcp.json bytes (or empty envelope when missing).
pub async fn get_mcp(State(state): State<AppState>) -> Result<Response, ApiResponseError> {
    let path = require_mcp_path(&state)?;
    let raw = McpFile::load_raw(path).map_err(|error| {
        error!(%error, "read mcp config failed");
        ApiResponseError::internal(format!("read mcp config: {error}"))
    })?;
    Ok(json_bytes_response(StatusCode::OK, raw))
}

/// `PUT /api/mcp` — validate then write raw request bytes verbatim.
pub async fn put_mcp(
    State(state): State<AppState>,
    body: Bytes,
) -> Result<Response, ApiResponseError> {
    let path = require_mcp_path(&state)?;
    McpFile::save_raw(path, &body).map_err(mcp_write_error)?;
    // Config change invalidates the lazy tools/list cache (S-PROF-TOOLS).
    if let Some(catalog) = state.tool_catalog.as_ref() {
        catalog.invalidate();
    }
    Ok(json_bytes_response(StatusCode::OK, body.to_vec()))
}

/// `PATCH /api/mcp/servers/{name}` — toggle one server's enabled flag.
pub async fn patch_mcp_server(
    State(state): State<AppState>,
    AxumPath(name): AxumPath<String>,
    body: Result<Json<PatchMcpServerRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let path = require_mcp_path(&state)?;
    if name.is_empty() {
        return Err(ApiResponseError::bad_request("missing server name"));
    }
    let enabled = request
        .enabled
        .ok_or_else(|| ApiResponseError::bad_request("missing 'enabled' field in request body"))?;

    let mut file = McpFile::load(path).map_err(|error| {
        error!(%error, "load mcp config failed");
        ApiResponseError::internal(format!("load mcp config: {error}"))
    })?;
    let Some(config) = file.mcp_servers.get_mut(&name) else {
        return Err(ApiResponseError::not_found(format!(
            "mcp server not found: {name}"
        )));
    };
    config.enabled = Some(enabled);
    file.save(path).map_err(|error| {
        error!(%error, "save mcp config failed");
        ApiResponseError::internal(format!("save mcp config: {error}"))
    })?;
    // Toggle changes the enabled set; drop cached tools/list snapshot.
    if let Some(catalog) = state.tool_catalog.as_ref() {
        catalog.invalidate();
    }

    Ok(Json(json!({
        "name": name,
        "enabled": enabled,
    })))
}

/// `GET /api/mcp/status` — on-demand health for every configured server.
pub async fn get_mcp_status(
    State(state): State<AppState>,
) -> Result<Json<Vec<ServerStatus>>, ApiResponseError> {
    let path = require_mcp_path(&state)?;
    let file = McpFile::load(path).map_err(|error| {
        error!(%error, "load mcp config failed");
        ApiResponseError::internal(format!("load mcp config: {error}"))
    })?;
    // Zero timeout selects the module default (2s), matching Go's CheckHealth(f, 0).
    Ok(Json(mcp::check_health(&file, Duration::ZERO)))
}

fn require_mcp_path(state: &AppState) -> Result<&Path, ApiResponseError> {
    state
        .mcp_config_path
        .as_deref()
        .ok_or_else(|| ApiResponseError::service_unavailable("mcp config not configured"))
}

fn mcp_write_error(error: McpError) -> ApiResponseError {
    match error {
        McpError::Json(error) => {
            ApiResponseError::bad_request(format!("invalid mcp config JSON: {error}"))
        }
        McpError::UnsupportedVersion(version) => {
            ApiResponseError::bad_request(format!("unsupported mcp config version: {version}"))
        }
        other => {
            error!(%other, "save mcp config failed");
            ApiResponseError::internal(format!("save mcp config: {other}"))
        }
    }
}

fn json_bytes_response(status: StatusCode, body: Vec<u8>) -> Response {
    (status, [(header::CONTENT_TYPE, "application/json")], body).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    fn state_with_mcp(dir: &tempfile::TempDir) -> AppState {
        crate::api::test_support::test_state_with_mcp(dir.path(), dir.path().join("mcp.json"))
    }

    #[tokio::test]
    async fn get_mcp_returns_empty_envelope_when_missing() {
        let dir = tempfile::tempdir().expect("temp");
        let response = crate::api::router(state_with_mcp(&dir))
            .oneshot(
                Request::builder()
                    .uri("/api/mcp")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::OK);
        let body = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .expect("body");
        assert_eq!(
            body.as_ref(),
            b"{\n  \"version\": 1,\n  \"mcpServers\": {}\n}\n"
        );
    }

    #[tokio::test]
    async fn put_mcp_rejects_invalid_json() {
        let dir = tempfile::tempdir().expect("temp");
        let response = crate::api::router(state_with_mcp(&dir))
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from("{not json"))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn put_and_patch_mcp_round_trip() {
        let dir = tempfile::tempdir().expect("temp");
        let raw = br#"{"version":1,"mcpServers":{"github":{"command":"echo","enabled":true}}}"#;
        let put = crate::api::router(state_with_mcp(&dir))
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/mcp")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(raw.as_slice()))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(put.status(), StatusCode::OK);

        let patch = crate::api::router(state_with_mcp(&dir))
            .oneshot(
                Request::builder()
                    .method("PATCH")
                    .uri("/api/mcp/servers/github")
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(Body::from(r#"{"enabled":false}"#))
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(patch.status(), StatusCode::OK);

        let file = McpFile::load(&dir.path().join("mcp.json")).expect("load");
        assert_eq!(file.mcp_servers["github"].enabled, Some(false));
    }

    #[tokio::test]
    async fn mcp_routes_return_503_when_unconfigured() {
        let dir = tempfile::tempdir().expect("temp");
        let state = crate::api::test_support::test_state(dir.path());
        let response = crate::api::router(state)
            .oneshot(
                Request::builder()
                    .uri("/api/mcp")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");
        assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE);
    }
}
