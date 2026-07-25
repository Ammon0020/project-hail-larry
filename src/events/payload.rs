//! SQLite payload JSON shape matching Go `events.eventPayload`.
//!
//! Variable event fields are stored in the `payload` TEXT column as JSON with
//! camelCase keys and `omitempty` semantics. Top-level columns hold only
//! `id`, `type`, `session_id`, and `timestamp` — matching Go exactly so
//! S-MIGRATE can open Go-created databases without payload drift.
//!
//! Fields present on the flat wire [`Event`] but **not** in Go's stored
//! payload (`stop_reason`, `execute_at`, `device_name`) are intentionally
//! omitted here. They round-trip as empty / year-1 defaults after load.

use serde::{Deserialize, Serialize};

use crate::interfaces::types::{Attachment, InjectedContext};
use crate::interfaces::Event;

/// On-disk JSON payload for one event row.
///
/// Field set and serde names must stay byte-compatible with Go's
/// `eventPayload` struct in `internal/events/events.go`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct StoredEventPayload {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub role: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub content: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub streaming: bool,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub summary: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub cwd: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub request_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_call_id: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub thought: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exit_code: Option<i32>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub injected_context: Vec<InjectedContext>,
}

impl StoredEventPayload {
    /// Project variable fields out of a flat event for storage.
    pub(crate) fn from_event(event: &Event) -> Self {
        Self {
            role: event.role.clone(),
            content: event.content.clone(),
            streaming: event.streaming,
            tool: event.tool.clone(),
            target: event.target.clone(),
            summary: event.summary.clone(),
            command: event.command.clone(),
            cwd: event.cwd.clone(),
            options: event.options.clone(),
            request_id: event.request_id.clone(),
            tool_kind: event.tool_kind.clone(),
            tool_call_id: event.tool_call_id.clone(),
            thought: event.thought,
            exit_code: event.exit_code,
            workspace_id: event.workspace_id.clone(),
            attachments: event.attachments.clone(),
            injected_context: event.injected_context.clone(),
        }
    }

    /// Apply stored payload fields onto an event that already has id/type/session/timestamp.
    pub(crate) fn apply_to(self, event: &mut Event) {
        event.role = self.role;
        event.content = self.content;
        event.streaming = self.streaming;
        event.tool = self.tool;
        event.target = self.target;
        event.summary = self.summary;
        event.command = self.command;
        event.cwd = self.cwd;
        event.options = self.options;
        event.request_id = self.request_id;
        event.tool_kind = self.tool_kind;
        event.tool_call_id = self.tool_call_id;
        event.thought = self.thought;
        event.exit_code = self.exit_code;
        event.workspace_id = self.workspace_id;
        event.attachments = self.attachments;
        event.injected_context = self.injected_context;
    }
}

/// Serialize a payload to the JSON string stored in the `payload` column.
pub(crate) fn encode_payload(event: &Event) -> Result<String, serde_json::Error> {
    serde_json::to_string(&StoredEventPayload::from_event(event))
}

/// Deserialize a payload column and merge it into `event`.
pub(crate) fn decode_payload(
    payload_str: &str,
    event: &mut Event,
) -> Result<(), serde_json::Error> {
    let payload: StoredEventPayload = serde_json::from_str(payload_str)?;
    payload.apply_to(event);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interfaces::types::{go_zero_time, EventType};
    use chrono::Utc;

    #[test]
    fn payload_omits_empty_fields() {
        let event = Event::new(0, EventType::PromptSubmitted, "s1", Utc::now());
        let json = encode_payload(&event).expect("encode");
        // Empty strings/false/nulls must be omitted (Go omitempty).
        assert_eq!(json, "{}");
    }

    #[test]
    fn payload_round_trip_tool_fields() {
        let mut event = Event::new(0, EventType::ToolCompleted, "s1", go_zero_time());
        event.tool = "edit_file".into();
        event.target = "server.js".into();
        event.summary = "Added handler".into();
        event.exit_code = Some(0);

        let json = encode_payload(&event).expect("encode");
        let mut back = Event::new(1, EventType::ToolCompleted, "s1", go_zero_time());
        decode_payload(&json, &mut back).expect("decode");

        assert_eq!(back.tool, "edit_file");
        assert_eq!(back.target, "server.js");
        assert_eq!(back.summary, "Added handler");
        assert_eq!(back.exit_code, Some(0));
    }

    #[test]
    fn payload_round_trips_injected_context() {
        let mut event = Event::new(0, EventType::PromptSubmitted, "s1", go_zero_time());
        event.injected_context = vec![InjectedContext {
            name: "demo1.html".into(),
            content: "<h1>Demo</h1>".into(),
        }];

        let json = encode_payload(&event).expect("encode");
        let mut back = Event::new(1, EventType::PromptSubmitted, "s1", go_zero_time());
        decode_payload(&json, &mut back).expect("decode");

        assert_eq!(back.injected_context, event.injected_context);
    }

    #[test]
    fn payload_uses_camel_case_keys() {
        let mut event = Event::new(0, EventType::PermissionRequested, "s1", Utc::now());
        event.request_id = "req-1".into();
        event.tool_kind = "execute".into();
        event.tool_call_id = "call-1".into();
        event.workspace_id = "ws-1".into();
        event.exit_code = Some(1);

        let json = encode_payload(&event).expect("encode");
        let v: serde_json::Value = serde_json::from_str(&json).expect("json");
        assert!(v.get("requestId").is_some());
        assert!(v.get("toolKind").is_some());
        assert!(v.get("toolCallId").is_some());
        assert!(v.get("workspaceId").is_some());
        assert!(v.get("exitCode").is_some());
        // snake_case must not appear.
        assert!(v.get("request_id").is_none());
        assert!(v.get("tool_kind").is_none());
    }
}
