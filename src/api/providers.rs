//! Session-scoped ACP LLM provider management handlers (Go `providers.go`).
//!
//! Unsupported capability maps to 501 with Go's handler wording; missing
//! sessions map to 404 via [`crate::interfaces::map_api_error`].

use std::collections::HashMap;

use axum::extract::rejection::JsonRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::interfaces::{ACPClient, AppError, ProviderInfo};

use super::{app_error, decode_json_body, ApiResponseError, AppState};

/// Accepted `apiType` values for PUT (Go `validLLMProtocols`).
const VALID_LLM_PROTOCOLS: &[&str] = &["anthropic", "openai", "azure", "vertex", "bedrock"];

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct SetProviderRequest {
    api_type: String,
    base_url: String,
    #[serde(default)]
    headers: HashMap<String, String>,
}

/// `GET /api/sessions/{id}/providers`
pub async fn list_providers(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ProviderInfo>>, ApiResponseError> {
    state
        .acp
        .list_providers(&session_id)
        .await
        .map(Json)
        .map_err(provider_error)
}

/// `PUT /api/sessions/{id}/providers/{provider_id}`
pub async fn set_provider(
    State(state): State<AppState>,
    Path((session_id, provider_id)): Path<(String, String)>,
    body: Result<Json<SetProviderRequest>, JsonRejection>,
) -> Result<Json<Value>, ApiResponseError> {
    let Json(request) = decode_json_body(body)?;
    if provider_id.is_empty() {
        return Err(ApiResponseError::bad_request("missing provider id in path"));
    }
    if request.api_type.is_empty() {
        return Err(ApiResponseError::bad_request("apiType is required"));
    }
    if !VALID_LLM_PROTOCOLS.contains(&request.api_type.as_str()) {
        return Err(ApiResponseError::bad_request(
            "invalid apiType: must be one of anthropic, openai, azure, vertex, bedrock",
        ));
    }
    if request.base_url.is_empty() {
        return Err(ApiResponseError::bad_request("baseUrl is required"));
    }

    state
        .acp
        .set_provider(
            &session_id,
            &provider_id,
            &request.api_type,
            &request.base_url,
            request.headers,
        )
        .await
        .map_err(provider_error)?;
    Ok(Json(json!({"status": "updated"})))
}

/// `DELETE /api/sessions/{id}/providers/{provider_id}`
pub async fn disable_provider(
    State(state): State<AppState>,
    Path((session_id, provider_id)): Path<(String, String)>,
) -> Result<Json<Value>, ApiResponseError> {
    if provider_id.is_empty() {
        return Err(ApiResponseError::bad_request("missing provider id in path"));
    }

    // Required-guard: refuse before calling disable (Go handler boundary).
    let providers = state
        .acp
        .list_providers(&session_id)
        .await
        .map_err(provider_error)?;
    if providers
        .iter()
        .any(|provider| provider.id == provider_id && provider.required)
    {
        return Err(ApiResponseError::bad_request(format!(
            "provider {provider_id} is required and cannot be disabled"
        )));
    }

    state
        .acp
        .disable_provider(&session_id, &provider_id)
        .await
        .map_err(provider_error)?;
    Ok(Json(json!({"status": "disabled"})))
}

/// Map ACP provider errors to Go's HTTP statuses and messages.
fn provider_error(error: AppError) -> ApiResponseError {
    match error {
        AppError::Unsupported(_) => ApiResponseError {
            status: StatusCode::NOT_IMPLEMENTED,
            // Go remaps the capability sentinel to this exact client message.
            message: "agent does not support provider management".into(),
        },
        other => app_error(other),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsupported_maps_to_501_with_go_wording() {
        let error = provider_error(AppError::unsupported(
            "agent does not support the providers capability",
        ));
        assert_eq!(error.status, StatusCode::NOT_IMPLEMENTED);
        assert_eq!(error.message, "agent does not support provider management");
    }

    #[test]
    fn not_found_maps_to_404() {
        let error = provider_error(AppError::not_found_kind("session"));
        assert_eq!(error.status, StatusCode::NOT_FOUND);
        assert_eq!(error.message, "session not found");
    }
}
