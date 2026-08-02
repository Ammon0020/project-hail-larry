//! Paired-device management and device-related pending actions.

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use chrono::Utc;
use serde_json::{json, Value};

use crate::interfaces::{Event, EventType, PENDING_ACTION_TYPE_REVOCATION};

use super::auth::device_id_from_request;
use super::{
    cancel_pending_action, decode_json_body, pairing_error, record_event, revocation_grace_period,
    ApiResponseError, AppState, CancelActionRequest,
};

pub(super) async fn list_devices(
    State(state): State<AppState>,
) -> Json<Vec<crate::interfaces::DeviceInfo>> {
    Json(state.pairing.list_devices())
}

/// `DELETE /api/devices/{id}` — immediate revoke (200) or grace-period pending (202).
pub(super) async fn revoke_device(
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Result<Response, ApiResponseError> {
    let grace = revocation_grace_period(&state);
    let requester = device_id_from_request(&headers);
    let info = state
        .pairing
        .request_revocation(&id, requester, grace)
        .map_err(pairing_error)?;

    if grace.is_zero() {
        return Ok((StatusCode::OK, Json(json!({"status": "revoked"}))).into_response());
    }

    // Broadcast so every connected device can surface and cancel the action.
    let mut event = Event::new(0, EventType::DeviceRevocationPending, "", Utc::now());
    event.target.clone_from(&info.device_id);
    event.device_name.clone_from(&info.device_name);
    event.request_id.clone_from(&info.id);
    event.command.clone_from(&info.requested_by);
    event.execute_at = info.execute_at;
    record_event(&state, event).await;

    Ok((StatusCode::ACCEPTED, Json(info)).into_response())
}

/// `POST /api/devices/cancel-revocation` — body `{"actionId":"..."}`.
///
/// A device that is the target of a pending revocation must not be able to
/// cancel its own revocation during the grace period.
pub(super) async fn cancel_revocation(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Result<Json<CancelActionRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let request = decode_json_body(body)?.0;
    let caller = device_id_from_request(&headers);
    if !caller.is_empty() {
        if let Some(action) = state
            .pairing
            .list_pending_actions()
            .into_iter()
            .find(|action| action.id == request.action_id)
        {
            if action.action_type == PENDING_ACTION_TYPE_REVOCATION && action.device_id == caller {
                return Err(ApiResponseError::forbidden(
                    "a device cannot cancel its own revocation",
                ));
            }
        }
    }
    cancel_pending_action(
        &state,
        request,
        |id| state.pairing.cancel_revocation(id),
        EventType::DeviceRevocationCancelled,
    )
    .await
}

/// `GET /api/pending-actions` — all grace-period pending actions.
pub(super) async fn list_pending_actions(
    State(state): State<AppState>,
) -> Json<Vec<crate::interfaces::PendingActionInfo>> {
    Json(state.pairing.list_pending_actions())
}
