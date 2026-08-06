use agent_client_protocol::schema::v1::{
    RequestPermissionOutcome, RequestPermissionRequest, RequestPermissionResponse,
    SelectedPermissionOutcome,
};
use uuid::Uuid;

use super::HandlerDeps;

/// Tool kinds for which the client synthesizes an "Always allow this tool type"
/// (`AllowToolKind`) option. This is intentionally conservative — a blanket
/// "always allow all shell commands" for `execute` would be a security risk
/// (one `allow_tool_kind` for `echo hello` would auto-approve `rm -rf /`).
/// `move`/`edit`/`read`/`search` are file-scoped operations where the worst case
/// is an unwanted file mutation, not arbitrary code execution.
const TOOL_KIND_ALLOWLIST: &[&str] = &["move", "edit", "read", "search"];

pub(in crate::acp::core) async fn request_permission(
    deps: HandlerDeps,
    request: RequestPermissionRequest,
) -> RequestPermissionResponse {
    let tool = request
        .tool_call
        .fields
        .title
        .clone()
        .unwrap_or_else(|| "Tool call".to_string());
    let tool_kind = request
        .tool_call
        .fields
        .kind
        .map(tool_kind_name)
        .unwrap_or_default();
    let command = request
        .tool_call
        .fields
        .raw_input
        .as_ref()
        .map_or_else(String::new, ToString::to_string);
    let target = request
        .tool_call
        .fields
        .locations
        .as_ref()
        .and_then(|locations| locations.first())
        .map_or_else(String::new, |location| {
            location.path.to_string_lossy().into_owned()
        });
    let mut options: Vec<crate::interfaces::PermissionDecision> = request
        .options
        .iter()
        .filter_map(|option| permission_decision(option.kind))
        .collect();
    let mut option_details: Vec<crate::interfaces::PermissionOptionInfo> = request
        .options
        .iter()
        .map(|option| crate::interfaces::PermissionOptionInfo {
            id: option.option_id.to_string(),
            name: option.name.clone(),
            kind: permission_kind_name(option.kind).to_string(),
        })
        .collect();

    // Synthesize an "Always allow this tool type" option for the conservative
    // allowlist of non-execute tool kinds. This is a client-only decision — no
    // ACP `PermissionOptionKind` maps to it. The option is appended to both
    // `options` (so `respond` validation accepts it) and `option_details` (so
    // the frontend can render it). When the user picks it, the manager records
    // a tool-kind-scoped policy and this handler responds to the ACP agent with
    // `AllowAlways` (the broadest ACP allow) so the agent proceeds.
    if TOOL_KIND_ALLOWLIST.contains(&tool_kind.as_str()) {
        options.push(crate::interfaces::PermissionDecision::AllowToolKind);
        option_details.push(crate::interfaces::PermissionOptionInfo {
            id: "allow_tool_kind".to_string(),
            name: "Always allow this tool type".to_string(),
            kind: "allow_tool_kind".to_string(),
        });
    }

    let permission = crate::interfaces::PermissionRequest {
        id: Uuid::new_v4().to_string(),
        // Agent session IDs are protocol transport identifiers. Permissions
        // belong to the local lifecycle entry so close clears its exact
        // pending prompts and durable policies.
        session_id: deps.local_session_id.clone(),
        tool,
        tool_kind,
        tool_call_id: request.tool_call.tool_call_id.to_string(),
        command,
        target,
        options,
        option_details,
    };
    match deps.permissions.request(permission).await {
        Ok(decision) => {
            // `AllowToolKind` is client-only — no ACP option maps to it. When
            // the user picks it, respond to the ACP agent with `AllowAlways`
            // (the broadest ACP allow) so the agent proceeds with the tool call.
            if decision == crate::interfaces::PermissionDecision::AllowToolKind {
                if let Some(option) = request.options.iter().find(|option| {
                    permission_decision(option.kind)
                        == Some(crate::interfaces::PermissionDecision::AllowAlways)
                }) {
                    return RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                        SelectedPermissionOutcome::new(option.option_id.clone()),
                    ));
                }
            }
            request
                .options
                .iter()
                .find(|option| permission_decision(option.kind) == Some(decision))
                .map_or_else(
                    || RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled),
                    |option| {
                        RequestPermissionResponse::new(RequestPermissionOutcome::Selected(
                            SelectedPermissionOutcome::new(option.option_id.clone()),
                        ))
                    },
                )
        }
        Err(error) => {
            tracing::warn!(error = %error, "ACP permission request cancelled");
            RequestPermissionResponse::new(RequestPermissionOutcome::Cancelled)
        }
    }
}

fn tool_kind_name(kind: agent_client_protocol::schema::v1::ToolKind) -> String {
    use agent_client_protocol::schema::v1::ToolKind;

    match kind {
        ToolKind::Read => "read",
        ToolKind::Edit => "edit",
        ToolKind::Delete => "delete",
        ToolKind::Move => "move",
        ToolKind::Search => "search",
        ToolKind::Execute => "execute",
        ToolKind::Think => "think",
        ToolKind::Fetch => "fetch",
        ToolKind::SwitchMode => "switch_mode",
        _ => "other",
    }
    .to_string()
}

fn permission_kind_name(
    kind: agent_client_protocol::schema::v1::PermissionOptionKind,
) -> &'static str {
    use agent_client_protocol::schema::v1::PermissionOptionKind;

    match kind {
        PermissionOptionKind::AllowOnce => "allow_once",
        PermissionOptionKind::AllowAlways => "allow_always",
        PermissionOptionKind::RejectOnce => "reject_once",
        PermissionOptionKind::RejectAlways => "reject_always",
        _ => "unknown",
    }
}

fn permission_decision(
    kind: agent_client_protocol::schema::v1::PermissionOptionKind,
) -> Option<crate::interfaces::PermissionDecision> {
    use crate::interfaces::PermissionDecision;
    use agent_client_protocol::schema::v1::PermissionOptionKind;

    match kind {
        PermissionOptionKind::AllowOnce => Some(PermissionDecision::AllowOnce),
        PermissionOptionKind::AllowAlways => Some(PermissionDecision::AllowAlways),
        PermissionOptionKind::RejectOnce => Some(PermissionDecision::Deny),
        PermissionOptionKind::RejectAlways => Some(PermissionDecision::RejectAlways),
        _ => None,
    }
}
