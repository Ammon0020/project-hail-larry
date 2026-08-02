//! Pending permission listing and response handlers.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::interfaces::{PermissionDecision, PermissionManager};

use super::{app_error, decode_json_body, ApiResponseError, AppState};

pub(super) async fn pending_permissions(
    State(state): State<AppState>,
) -> Json<Vec<crate::interfaces::PermissionRequest>> {
    Json(state.permissions.get_pending())
}

#[derive(Deserialize)]
pub(super) struct RespondPermissionRequest {
    decision: String,
}

pub(super) async fn respond_permission(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: Result<Json<RespondPermissionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    let decision = serde_json::from_value::<PermissionDecision>(Value::String(request.decision))
        .map_err(|_| ApiResponseError::bad_request("invalid permission decision"))?;
    state
        .permissions
        .respond(&id, decision)
        .await
        .map_err(app_error)?;
    Ok(Json(json!({"status": "responded"})))
}
