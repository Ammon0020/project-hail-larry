//! Unstable ACP LLM provider management (Go `ListProviders` / `SetProvider` /
//! `DisableProvider` + model-config discovery for `SwitchModel`).
//!
//! ## SDK approach
//!
//! `agent-client-protocol` **1.2.0 does not forward** the schema feature
//! `unstable_llm_providers` (its `unstable` umbrella omits it). Typed
//! `providers/list|set|disable` RPCs therefore cannot go through the SDK's
//! generated request enum.
//!
//! We use a **hand-rolled JSON-RPC fallback**: local request/response types
//! that implement [`JsonRpcRequest`] / [`JsonRpcResponse`] via the SDK derive
//! macros, with Go-compatible wire field names (`id`, not the newer schema's
//! `providerId`). Capability detection still uses schema feature unification
//! (`agent-client-protocol-schema` / `unstable_llm_providers`) so
//! `InitializeResponse.agent_capabilities.providers` is not stripped.
//!
//! List responses accept both `id` and `providerId` for forward compatibility.

use std::collections::{HashMap, HashSet};

use agent_client_protocol::schema::v1::{
    SessionConfigKind, SessionConfigOption, SessionConfigOptionCategory,
    SessionConfigSelectOptions, SetSessionConfigOptionRequest,
};
use agent_client_protocol::{Agent, ConnectionTo, JsonRpcRequest, JsonRpcResponse};
use serde::{Deserialize, Serialize};

use crate::config::AgentModel;
use crate::interfaces::{AppError, ProviderCurrentConfig, ProviderInfo};

/// Message for [`AppError::Unsupported`] when the agent omitted providers caps.
///
/// Matches Go `ErrProvidersUnsupported`. REST remaps the wording for 501.
pub const PROVIDERS_UNSUPPORTED_MSG: &str = "agent does not support the providers capability";

/// Message when live model switch is impossible without rebind (CONTEXT owns rebind).
pub const MODEL_SWITCH_UNSUPPORTED_MSG: &str = "agent does not advertise a model config option; \
live model switch requires session/set_config_option (rebind fallback deferred to S-ACP-CONTEXT)";

/// Per-session capability cache captured from `initialize`.
///
/// Provider RPCs gate on `providers_supported`. Context injection gates on
/// `embedded_context`. Session-history stories (BROWSE/OPEN/FALLBACK) gate on
/// the `can_*` fields — live-only until epic Q8 (cold-start probe) locks.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionCaps {
    /// `agentCapabilities.providers != null` from Initialize.
    pub providers_supported: bool,
    /// `promptCapabilities.embeddedContext` from Initialize (CONTEXT).
    pub embedded_context: bool,
    /// `sessionCapabilities.list` present (`{}` or richer).
    pub can_list_sessions: bool,
    /// Top-level `agentCapabilities.loadSession`.
    pub can_load_session: bool,
    /// `sessionCapabilities.resume` present.
    pub can_resume_session: bool,
    /// `sessionCapabilities.close` present.
    pub can_close_session: bool,
    /// `sessionCapabilities.delete` present.
    pub can_delete_session: bool,
}

impl SessionCaps {
    /// Project cached initialize caps into the stable REST/UI shape.
    ///
    /// When `available` is false (dormant session, no live initialize), all
    /// capability booleans are false — callers must not treat that as
    /// "agent lacks list/load"; cold-start policy is still Decision Needed (Q8).
    #[must_use]
    pub fn to_history_capabilities(self, available: bool) -> crate::interfaces::SessionHistoryCapabilities {
        if !available {
            return crate::interfaces::SessionHistoryCapabilities::unavailable();
        }
        crate::interfaces::SessionHistoryCapabilities {
            available: true,
            can_list_sessions: self.can_list_sessions,
            can_load_session: self.can_load_session,
            can_resume_session: self.can_resume_session,
            can_close_session: self.can_close_session,
            can_delete_session: self.can_delete_session,
        }
    }
}

/// Fail with 501-class unsupported when the agent did not advertise providers.
pub fn require_providers_supported(caps: SessionCaps) -> Result<(), AppError> {
    if caps.providers_supported {
        Ok(())
    } else {
        tracing::info!("providers RPC gated: agent did not advertise providers capability");
        Err(AppError::unsupported(PROVIDERS_UNSUPPORTED_MSG))
    }
}

/// Project ACP provider list entries into the interface-layer DTO.
///
/// Returns an empty `Vec` (never `null`/`None`) so REST serializes `[]`.
#[must_use]
pub fn to_interface_providers(providers: Vec<WireProviderInfo>) -> Vec<ProviderInfo> {
    providers
        .into_iter()
        .map(|p| ProviderInfo {
            id: p.id,
            required: p.required,
            supported: p.supported,
            current: p.current.map(|c| ProviderCurrentConfig {
                api_type: c.api_type,
                base_url: c.base_url,
            }),
        })
        .collect()
}

/// Wire shape for one provider in `providers/list` (Go `UnstableProviderInfo`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WireProviderInfo {
    /// Provider id. Accept newer schema `providerId` when deserializing.
    #[serde(alias = "providerId")]
    pub id: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub supported: Vec<String>,
    /// Omitted/null means the provider is disabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<WireProviderCurrent>,
}

/// Non-secret routing config on the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WireProviderCurrent {
    pub api_type: String,
    pub base_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "providers/list", response = ListProvidersWireResponse)]
struct ListProvidersWireRequest {}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcResponse)]
struct ListProvidersWireResponse {
    #[serde(default)]
    providers: Vec<WireProviderInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "providers/set", response = EmptyProvidersResponse)]
#[serde(rename_all = "camelCase")]
struct SetProviderWireRequest {
    id: String,
    api_type: String,
    base_url: String,
    /// Omitted when empty so the wire matches Go's `omitempty` nil map.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    headers: HashMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, JsonRpcRequest)]
#[request(method = "providers/disable", response = EmptyProvidersResponse)]
struct DisableProviderWireRequest {
    id: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, JsonRpcResponse)]
struct EmptyProvidersResponse {}

/// Call unstable `providers/list` and project to interface DTOs.
pub async fn rpc_list_providers(cx: &ConnectionTo<Agent>) -> Result<Vec<ProviderInfo>, AppError> {
    let response = cx
        .send_request(ListProvidersWireRequest {})
        .block_task()
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "providers/list failed");
            AppError::internal(format!("providers/list: {error}"))
        })?;
    Ok(to_interface_providers(response.providers))
}

/// Call unstable `providers/set` (full replace for one provider id).
pub async fn rpc_set_provider(
    cx: &ConnectionTo<Agent>,
    id: String,
    api_type: String,
    base_url: String,
    headers: HashMap<String, String>,
) -> Result<(), AppError> {
    if id.is_empty() {
        return Err(AppError::validation("provider id is required"));
    }
    if api_type.is_empty() {
        return Err(AppError::validation("apiType is required"));
    }
    if base_url.is_empty() {
        return Err(AppError::validation("baseUrl is required"));
    }
    cx.send_request(SetProviderWireRequest {
        id,
        api_type,
        base_url,
        headers,
    })
    .block_task()
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "providers/set failed");
        AppError::internal(format!("providers/set: {error}"))
    })?;
    Ok(())
}

/// Call unstable `providers/disable`.
pub async fn rpc_disable_provider(cx: &ConnectionTo<Agent>, id: String) -> Result<(), AppError> {
    if id.is_empty() {
        return Err(AppError::validation("provider id is required"));
    }
    cx.send_request(DisableProviderWireRequest { id })
        .block_task()
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "providers/disable failed");
            AppError::internal(format!("providers/disable: {error}"))
        })?;
    Ok(())
}

/// Live model switch via `session/set_config_option`.
pub async fn rpc_set_model_config(
    cx: &ConnectionTo<Agent>,
    agent_session_id: &agent_client_protocol::schema::v1::SessionId,
    config_id: &str,
    model_id: &str,
) -> Result<(), AppError> {
    let request = SetSessionConfigOptionRequest::new(
        agent_session_id.clone(),
        config_id.to_string(),
        model_id, // &str → SessionConfigOptionValue via From<&str>
    );
    cx.send_request(request)
        .block_task()
        .await
        .map_err(|error| {
            tracing::error!(
                error = %error,
                config_id,
                model_id,
                "session/set_config_option (model) failed"
            );
            AppError::internal(format!("set model config option: {error}"))
        })?;
    Ok(())
}

/// Locate the model selector config option id (Go `findModelConfigID`).
///
/// Priority: category `model` → id `model` → name contains "model" → value
/// matches a known registry model id. Returns `None` when no option matches.
#[must_use]
pub fn find_model_config_id(
    opts: &[SessionConfigOption],
    known_models: &[AgentModel],
) -> Option<String> {
    let known: HashSet<&str> = known_models.iter().map(|m| m.id.as_str()).collect();

    // Pass 1: explicit category == model (spec-preferred).
    for opt in opts {
        if matches!(opt.category, Some(SessionConfigOptionCategory::Model))
            && matches!(opt.kind, SessionConfigKind::Select(_))
        {
            return Some(opt.id.to_string());
        }
    }
    // Pass 2: conventional id "model".
    for opt in opts {
        if opt.id.0.as_ref() == "model" && matches!(opt.kind, SessionConfigKind::Select(_)) {
            return Some(opt.id.to_string());
        }
    }
    // Pass 3: name contains "model" (case-insensitive).
    for opt in opts {
        if opt.name.to_ascii_lowercase().contains("model")
            && matches!(opt.kind, SessionConfigKind::Select(_))
        {
            return Some(opt.id.to_string());
        }
    }
    // Pass 4: current or listed value matches a known model id.
    if known.is_empty() {
        return None;
    }
    for opt in opts {
        let SessionConfigKind::Select(select) = &opt.kind else {
            continue;
        };
        if known.contains(select.current_value.0.as_ref()) {
            return Some(opt.id.to_string());
        }
        let hit = match &select.options {
            SessionConfigSelectOptions::Ungrouped(options) => {
                options.iter().any(|o| known.contains(o.value.0.as_ref()))
            }
            SessionConfigSelectOptions::Grouped(groups) => groups
                .iter()
                .flat_map(|g| g.options.iter())
                .any(|o| known.contains(o.value.0.as_ref())),
            _ => false,
        };
        if hit {
            return Some(opt.id.to_string());
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_client_protocol::schema::v1::{SessionConfigOption, SessionConfigSelectOption};

    #[test]
    fn require_providers_supported_gates() {
        assert!(require_providers_supported(SessionCaps {
            providers_supported: true,
            ..SessionCaps::default()
        })
        .is_ok());
        let err = require_providers_supported(SessionCaps::default())
            .expect_err("unsupported when false");
        match err {
            AppError::Unsupported(msg) => assert_eq!(msg, PROVIDERS_UNSUPPORTED_MSG),
            other => panic!("expected Unsupported, got {other}"),
        }
    }

    #[test]
    fn history_caps_projection_with_list_and_load() {
        let caps = SessionCaps {
            providers_supported: false,
            embedded_context: true,
            can_list_sessions: true,
            can_load_session: true,
            can_resume_session: false,
            can_close_session: false,
            can_delete_session: false,
        };
        let projected = caps.to_history_capabilities(true);
        assert!(projected.available);
        assert!(projected.can_list_sessions);
        assert!(projected.can_load_session);
        assert!(!projected.can_resume_session);
        assert!(!projected.can_close_session);
        assert!(!projected.can_delete_session);
        let json = serde_json::to_value(projected).expect("serialize");
        assert_eq!(json["available"], true);
        assert_eq!(json["canListSessions"], true);
        assert_eq!(json["canLoadSession"], true);
        assert_eq!(json["canResumeSession"], false);
    }

    #[test]
    fn history_caps_projection_without_list_or_load() {
        let caps = SessionCaps::default();
        let projected = caps.to_history_capabilities(true);
        assert!(projected.available);
        assert!(!projected.can_list_sessions);
        assert!(!projected.can_load_session);
        assert!(!projected.can_resume_session);
        // FALLBACK story can gate on: available && !canList && !canLoad
        assert!(
            projected.available && !projected.can_list_sessions && !projected.can_load_session,
            "agents lacking list+load must be identifiable"
        );
    }

    #[test]
    fn history_caps_unavailable_when_not_live() {
        let caps = SessionCaps {
            providers_supported: true,
            embedded_context: true,
            can_list_sessions: true,
            can_load_session: true,
            can_resume_session: true,
            can_close_session: true,
            can_delete_session: true,
        };
        let projected = caps.to_history_capabilities(false);
        assert!(!projected.available);
        assert!(!projected.can_list_sessions);
        assert!(!projected.can_load_session);
        assert_eq!(
            projected,
            crate::interfaces::SessionHistoryCapabilities::unavailable()
        );
    }

    #[test]
    fn to_interface_providers_empty_is_non_null_slice() {
        let got = to_interface_providers(Vec::new());
        assert!(got.is_empty());
        // serde_json would emit [] for Vec, never null — mirrors Go empty slice.
        let json = serde_json::to_value(&got).expect("serialize");
        assert!(json.is_array());
    }

    #[test]
    fn to_interface_providers_converts_shapes() {
        let got = to_interface_providers(vec![
            WireProviderInfo {
                id: "main".into(),
                required: true,
                supported: vec!["anthropic".into(), "openai".into()],
                current: Some(WireProviderCurrent {
                    api_type: "anthropic".into(),
                    base_url: "https://api.anthropic.com".into(),
                }),
            },
            WireProviderInfo {
                id: "openai".into(),
                required: false,
                supported: vec!["openai".into()],
                current: None,
            },
        ]);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].id, "main");
        assert!(got[0].required);
        assert_eq!(
            got[0].current.as_ref().map(|c| c.api_type.as_str()),
            Some("anthropic")
        );
        assert!(got[1].current.is_none());
    }

    #[test]
    fn wire_provider_accepts_provider_id_alias() {
        let raw = serde_json::json!({
            "providerId": "main",
            "required": true,
            "supported": ["anthropic"],
            "current": {"apiType": "anthropic", "baseUrl": "https://api.anthropic.com"}
        });
        let parsed: WireProviderInfo = serde_json::from_value(raw).expect("parse");
        assert_eq!(parsed.id, "main");
        let as_go = serde_json::to_value(&parsed).expect("serialize");
        assert_eq!(as_go["id"], "main");
        assert!(as_go.get("providerId").is_none());
    }

    #[test]
    fn wire_provider_matches_golden_id_shape() {
        let info = WireProviderInfo {
            id: "main".into(),
            required: true,
            supported: vec!["anthropic".into(), "openai".into()],
            current: Some(WireProviderCurrent {
                api_type: "anthropic".into(),
                base_url: "https://api.anthropic.com".into(),
            }),
        };
        let projected = to_interface_providers(vec![info]);
        let json = serde_json::to_value(&projected[0]).expect("serialize");
        assert_eq!(json["id"], "main");
        assert_eq!(json["required"], true);
        assert_eq!(json["supported"][0], "anthropic");
        assert_eq!(json["current"]["apiType"], "anthropic");
        assert_eq!(json["current"]["baseUrl"], "https://api.anthropic.com");
    }

    #[test]
    fn set_provider_omits_empty_headers() {
        let req = SetProviderWireRequest {
            id: "main".into(),
            api_type: "openai".into(),
            base_url: "https://x".into(),
            headers: HashMap::new(),
        };
        let json = serde_json::to_value(&req).expect("serialize");
        assert!(json.get("headers").is_none());
        assert_eq!(json["id"], "main");
    }

    #[test]
    fn find_model_config_id_category_match() {
        let opts = vec![
            SessionConfigOption::select(
                "thought",
                "Thought",
                "low",
                vec![SessionConfigSelectOption::new("low", "Low")],
            )
            .category(SessionConfigOptionCategory::ThoughtLevel),
            SessionConfigOption::select(
                "model",
                "Model",
                "claude-sonnet-4",
                vec![SessionConfigSelectOption::new("claude-sonnet-4", "Sonnet")],
            )
            .category(SessionConfigOptionCategory::Model),
        ];
        assert_eq!(find_model_config_id(&opts, &[]).as_deref(), Some("model"));
    }

    #[test]
    fn find_model_config_id_id_convention() {
        let opts = vec![SessionConfigOption::select(
            "model",
            "Provider",
            "gpt-4o",
            vec![SessionConfigSelectOption::new("gpt-4o", "4o")],
        )];
        assert_eq!(find_model_config_id(&opts, &[]).as_deref(), Some("model"));
    }

    #[test]
    fn find_model_config_id_known_value_fallback() {
        let known = [AgentModel {
            id: "mistral-medium-3.5".into(),
            name: "Mistral Medium".into(),
        }];
        let opts = vec![SessionConfigOption::select(
            "selector",
            "Chooser",
            "mistral-medium-3.5",
            vec![SessionConfigSelectOption::new(
                "mistral-medium-3.5",
                "Medium",
            )],
        )];
        assert_eq!(
            find_model_config_id(&opts, &known).as_deref(),
            Some("selector")
        );
    }

    #[test]
    fn find_model_config_id_none_when_absent() {
        let opts = vec![SessionConfigOption::select(
            "thought",
            "Thought",
            "low",
            vec![SessionConfigSelectOption::new("low", "Low")],
        )
        .category(SessionConfigOptionCategory::ThoughtLevel)];
        assert!(find_model_config_id(&opts, &[]).is_none());
    }
}
