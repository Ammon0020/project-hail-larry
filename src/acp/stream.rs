//! Translate ACP session updates into the app's typed durable-event payloads.
//!
//! This boundary deliberately accepts only display-safe fields. Unsupported
//! protocol updates are logged by kind only: agent-supplied payloads can contain
//! prompts, command arguments, or credentials and must not be copied to logs.

use agent_client_protocol::schema::v1::{
    ContentBlock, Plan, PlanEntryStatus, SessionUpdate, ToolCall, ToolCallContent, ToolCallStatus,
    ToolKind, UsageUpdate,
};
use tracing::warn;

use crate::interfaces::EventPayload;

/// Convert a supported ACP session update to a typed app event payload.
///
/// User chunks duplicate the locally-created `PromptSubmitted` event and are
/// therefore intentionally ignored. Every other unmapped update is made
/// visible to operators without logging untrusted content.
pub fn session_update_to_payload(update: &SessionUpdate) -> Option<EventPayload> {
    match update {
        SessionUpdate::AgentMessageChunk(chunk) => text_chunk(&chunk.content, false),
        SessionUpdate::AgentThoughtChunk(chunk) => text_chunk(&chunk.content, true),
        SessionUpdate::ToolCall(tool_call) => Some(tool_started(tool_call)),
        SessionUpdate::ToolCallUpdate(update) => Some(tool_completed(update)),
        SessionUpdate::Plan(plan) => Some(EventPayload::PlanUpdated {
            content: plan_summary(plan),
        }),
        SessionUpdate::UserMessageChunk(_) => None,
        // These protocol capabilities have no app event contract yet. Keep the
        // kind observable while redacting the opaque agent-supplied body.
        SessionUpdate::AvailableCommandsUpdate(_) => unmapped_update("available_commands_update"),
        SessionUpdate::CurrentModeUpdate(_) => unmapped_update("current_mode_update"),
        SessionUpdate::ConfigOptionUpdate(_) => unmapped_update("config_option_update"),
        SessionUpdate::SessionInfoUpdate(_) => unmapped_update("session_info_update"),
        SessionUpdate::UsageUpdate(update) => Some(usage_update(update)),
        _ => unmapped_update("unrecognized"),
    }
}

/// Build a streaming text event and make non-text chunks observable.
fn text_chunk(content: &ContentBlock, thought: bool) -> Option<EventPayload> {
    match content {
        ContentBlock::Text(text) => Some(EventPayload::StreamUpdate {
            role: "agent".to_string(),
            content: text.text.clone(),
            streaming: true,
            thought,
            stop_reason: String::new(),
        }),
        // A future ContentBlock variant is deliberately handled by the
        // non-exhaustive fallback so protocol upgrades cannot lose data silently.
        ContentBlock::Image(_)
        | ContentBlock::Audio(_)
        | ContentBlock::ResourceLink(_)
        | ContentBlock::Resource(_) => unmapped_update("non_text_stream_chunk"),
        _ => unmapped_update("unrecognized_stream_chunk"),
    }
}

/// Translate a newly-announced ACP tool call.
fn tool_started(tool_call: &ToolCall) -> EventPayload {
    EventPayload::ToolStarted {
        tool: tool_call.title.clone(),
        tool_kind: tool_kind_name(tool_call.kind).to_string(),
        tool_call_id: tool_call.tool_call_id.to_string(),
        target: first_location(&tool_call.locations),
        command: tool_call
            .raw_input
            .as_ref()
            .map_or_else(String::new, ToString::to_string),
        // Go exposes the initial ACP status in `summary`. Keep it as a typed
        // field so the wire adapter retains that established UI contract.
        summary: tool_status_name(tool_call.status).to_string(),
    }
}

/// Translate a progress/result update for an existing ACP tool call.
fn tool_completed(update: &agent_client_protocol::schema::v1::ToolCallUpdate) -> EventPayload {
    let fields = &update.fields;
    let mut content = fields
        .content
        .as_deref()
        .map_or_else(String::new, tool_content_summary);
    if content.is_empty() {
        content = fields
            .raw_output
            .as_ref()
            .map_or_else(String::new, ToString::to_string);
    }

    EventPayload::ToolCompleted {
        // ToolCallUpdate title is optional. The Go transport leaves `tool`
        // empty when no title is supplied, so preserve that wire behavior.
        tool: fields.title.clone().unwrap_or_default(),
        tool_kind: fields
            .kind
            .map_or_else(String::new, |kind| tool_kind_name(kind).to_string()),
        tool_call_id: update.tool_call_id.to_string(),
        target: fields
            .locations
            .as_deref()
            .map_or_else(String::new, first_location),
        summary: fields
            .status
            .map_or_else(String::new, |status| tool_status_name(status).to_string()),
        // Go puts text/diff/raw output in `content`. Preserve this in the typed
        // payload rather than overloading `summary`, which carries the status.
        content,
        exit_code: None,
    }
}

/// Render ACP tool result blocks into the compact legacy tool-card content.
fn tool_content_summary(blocks: &[ToolCallContent]) -> String {
    blocks
        .iter()
        .filter_map(|block| match block {
            ToolCallContent::Content(content) => match &content.content {
                ContentBlock::Text(text) => Some(text.text.clone()),
                _ => None,
            },
            ToolCallContent::Diff(diff) => Some(format!(
                "--- {}\n{}\n+++\n{}",
                diff.path.display(),
                diff.old_text.as_deref().unwrap_or_default(),
                diff.new_text
            )),
            ToolCallContent::Terminal(terminal) => {
                Some(format!("[terminal {}]", terminal.terminal_id))
            }
            _ => None,
        })
        .filter(|content| !content.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

/// Render a complete ACP plan using the Go-compatible `status: content` form.
fn plan_summary(plan: &Plan) -> String {
    plan.entries
        .iter()
        .map(|entry| format!("{}: {}", plan_status_name(&entry.status), entry.content))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Translate an ACP `usage_update` into a typed app event. The agent reports
/// cumulative context usage and optional session cost; we forward both so the
/// client can render a context-fill ring and cost display.
fn usage_update(update: &UsageUpdate) -> EventPayload {
    let (cost_amount, cost_currency) = update.cost.as_ref().map_or((None, String::new()), |cost| {
        (Some(cost.amount), cost.currency.clone())
    });
    EventPayload::UsageUpdated {
        used: update.used,
        size: update.size,
        cost_amount,
        cost_currency,
    }
}

fn first_location(locations: &[agent_client_protocol::schema::v1::ToolCallLocation]) -> String {
    locations.first().map_or_else(String::new, |location| {
        location.path.to_string_lossy().into_owned()
    })
}

fn tool_kind_name(kind: ToolKind) -> &'static str {
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
        ToolKind::Other | _ => "other",
    }
}

fn tool_status_name(status: ToolCallStatus) -> &'static str {
    match status {
        ToolCallStatus::Pending => "pending",
        ToolCallStatus::InProgress => "in_progress",
        ToolCallStatus::Completed => "completed",
        ToolCallStatus::Failed => "failed",
        _ => "unknown",
    }
}

fn plan_status_name(status: &PlanEntryStatus) -> &'static str {
    match status {
        PlanEntryStatus::Pending => "pending",
        PlanEntryStatus::InProgress => "in_progress",
        PlanEntryStatus::Completed => "completed",
        _ => "unknown",
    }
}

/// Log only a stable update category, never the untrusted ACP payload.
fn unmapped_update(kind: &'static str) -> Option<EventPayload> {
    warn!(
        update_kind = kind,
        "ACP session update is unsupported and was not persisted"
    );
    None
}

#[cfg(test)]
mod tests {
    use agent_client_protocol::schema::v1::{
        ContentBlock, ContentChunk, CurrentModeUpdate, Plan, PlanEntry, PlanEntryPriority,
        PlanEntryStatus, SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallLocation,
        ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields, ToolKind,
    };

    use super::session_update_to_payload;
    use crate::interfaces::EventPayload;

    #[test]
    fn agent_message_chunk_becomes_stream_update() {
        let update = SessionUpdate::AgentMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("hello"),
        )));

        assert_eq!(
            session_update_to_payload(&update),
            Some(EventPayload::StreamUpdate {
                role: "agent".to_string(),
                content: "hello".to_string(),
                streaming: true,
                thought: false,
                stop_reason: String::new(),
            })
        );
    }

    #[test]
    fn agent_thought_chunk_marks_thought() {
        let update = SessionUpdate::AgentThoughtChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("considering options"),
        )));

        assert!(matches!(
            session_update_to_payload(&update),
            Some(EventPayload::StreamUpdate { thought: true, .. })
        ));
    }

    #[test]
    fn tool_call_preserves_go_tool_started_fields() {
        let tool = ToolCall::new("call-1", "Run tests")
            .kind(ToolKind::Execute)
            .status(ToolCallStatus::InProgress)
            .locations(vec![ToolCallLocation::new("/workspace/Cargo.toml")])
            .raw_input(serde_json::json!({"command": "cargo test -q"}));

        assert_eq!(
            session_update_to_payload(&SessionUpdate::ToolCall(tool)),
            Some(EventPayload::ToolStarted {
                tool: "Run tests".to_string(),
                tool_kind: "execute".to_string(),
                tool_call_id: "call-1".to_string(),
                target: "/workspace/Cargo.toml".to_string(),
                command: r#"{"command":"cargo test -q"}"#.to_string(),
                summary: "in_progress".to_string(),
            })
        );
    }

    #[test]
    fn tool_call_update_renders_content_and_status() {
        let fields = ToolCallUpdateFields::new()
            .kind(ToolKind::Edit)
            .status(ToolCallStatus::Completed)
            .locations(vec![ToolCallLocation::new("/workspace/src/lib.rs")])
            .content(vec![ToolCallContent::from(ContentBlock::Text(
                TextContent::new("updated"),
            ))]);
        let update = SessionUpdate::ToolCallUpdate(ToolCallUpdate::new("call-1", fields));

        assert_eq!(
            session_update_to_payload(&update),
            Some(EventPayload::ToolCompleted {
                tool: String::new(),
                tool_kind: "edit".to_string(),
                tool_call_id: "call-1".to_string(),
                target: "/workspace/src/lib.rs".to_string(),
                summary: "completed".to_string(),
                content: "updated".to_string(),
                exit_code: None,
            })
        );
    }

    #[test]
    fn plan_renders_status_and_content() {
        let plan = Plan::new(vec![PlanEntry::new(
            "Implement durable events",
            PlanEntryPriority::High,
            PlanEntryStatus::InProgress,
        )]);

        assert_eq!(
            session_update_to_payload(&SessionUpdate::Plan(plan)),
            Some(EventPayload::PlanUpdated {
                content: "in_progress: Implement durable events".to_string(),
            })
        );
    }

    #[test]
    fn user_message_chunk_is_not_duplicated() {
        let update = SessionUpdate::UserMessageChunk(ContentChunk::new(ContentBlock::Text(
            TextContent::new("already persisted"),
        )));

        assert_eq!(session_update_to_payload(&update), None);
    }

    #[test]
    fn unmapped_update_returns_none_after_redacted_warning() {
        let update = SessionUpdate::CurrentModeUpdate(CurrentModeUpdate::new("plan"));

        assert_eq!(session_update_to_payload(&update), None);
    }

    #[test]
    fn usage_update_becomes_usage_updated_payload() {
        use agent_client_protocol::schema::v1::{Cost, UsageUpdate as AcpUsageUpdate};

        // With cost — both token counts and cost fields are forwarded.
        let with_cost = SessionUpdate::UsageUpdate(
            AcpUsageUpdate::new(53_000, 200_000).cost(Cost::new(0.045, "USD")),
        );
        assert_eq!(
            session_update_to_payload(&with_cost),
            Some(EventPayload::UsageUpdated {
                used: 53_000,
                size: 200_000,
                cost_amount: Some(0.045),
                cost_currency: "USD".to_string(),
            })
        );

        // Without cost — cost_amount is None and currency is empty.
        let no_cost = SessionUpdate::UsageUpdate(AcpUsageUpdate::new(1_000, 8_000));
        assert_eq!(
            session_update_to_payload(&no_cost),
            Some(EventPayload::UsageUpdated {
                used: 1_000,
                size: 8_000,
                cost_amount: None,
                cost_currency: String::new(),
            })
        );
    }
}
