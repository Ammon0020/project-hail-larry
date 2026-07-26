//! Wire adapter: typed internal events ↔ flat Go JSON event shape.
//!
//! Rust code prefers [`TypedEvent`] / [`EventPayload`] for type safety. The
//! public REST/WS contract remains the flat Go JSON form captured by S-CONTRACT
//! golden fixtures. This module is the only place that projects between them.
//!
//! Do not expose a serde-enum representation as the public contract.

use chrono::{DateTime, Utc};

use super::types::{go_zero_time, Event, EventMeta, EventPayload, EventType, TypedEvent};

/// Project a typed internal event into the flat Go-compatible wire struct.
///
/// Fields not set by the payload remain empty / default and are dropped by
/// `omitempty` on serialization (except `execute_at`, which Go always emits).
#[must_use]
pub fn typed_event_to_wire(typed: &TypedEvent) -> Event {
    let mut event = Event {
        id: typed.meta.id,
        event_type: typed.payload.event_type(),
        session_id: typed.meta.session_id.clone(),
        timestamp: typed.meta.timestamp,
        role: String::new(),
        content: String::new(),
        streaming: false,
        tool: String::new(),
        target: String::new(),
        summary: String::new(),
        command: String::new(),
        cwd: String::new(),
        options: Vec::new(),
        request_id: String::new(),
        tool_kind: String::new(),
        tool_call_id: String::new(),
        thought: false,
        exit_code: None,
        stop_reason: String::new(),
        workspace_id: String::new(),
        attachments: Vec::new(),
        injected_context: Vec::new(),
        execute_at: go_zero_time(),
        device_name: String::new(),
    };

    apply_payload(&mut event, &typed.payload);
    event
}

/// Populate flat event fields from a typed payload.
// Single linear payload adapter — splitting would obscure the variant mapping.
#[allow(clippy::too_many_lines)]
fn apply_payload(event: &mut Event, payload: &EventPayload) {
    match payload {
        EventPayload::PromptSubmitted {
            role,
            content,
            attachments,
            injected_context,
        } => {
            event.role.clone_from(role);
            event.content.clone_from(content);
            event.attachments.clone_from(attachments);
            event.injected_context.clone_from(injected_context);
        }
        EventPayload::ResponseStarted { role } => {
            event.role.clone_from(role);
        }
        EventPayload::StreamUpdate {
            role,
            content,
            streaming,
            thought,
            stop_reason,
        } => {
            event.role.clone_from(role);
            event.content.clone_from(content);
            event.streaming = *streaming;
            event.thought = *thought;
            event.stop_reason.clone_from(stop_reason);
        }
        EventPayload::ToolStarted {
            tool,
            tool_kind,
            tool_call_id,
            target,
            command,
            summary,
        } => {
            event.tool.clone_from(tool);
            event.tool_kind.clone_from(tool_kind);
            event.tool_call_id.clone_from(tool_call_id);
            event.target.clone_from(target);
            event.command.clone_from(command);
            event.summary.clone_from(summary);
        }
        EventPayload::ToolCompleted {
            tool,
            tool_kind,
            tool_call_id,
            target,
            summary,
            content,
            exit_code,
        } => {
            event.tool.clone_from(tool);
            event.tool_kind.clone_from(tool_kind);
            event.tool_call_id.clone_from(tool_call_id);
            event.target.clone_from(target);
            event.summary.clone_from(summary);
            event.content.clone_from(content);
            event.exit_code = *exit_code;
        }
        EventPayload::PermissionRequested {
            request_id,
            tool,
            tool_kind,
            target,
            command,
            options,
        } => {
            event.request_id.clone_from(request_id);
            event.tool.clone_from(tool);
            event.tool_kind.clone_from(tool_kind);
            event.target.clone_from(target);
            event.command.clone_from(command);
            event.options.clone_from(options);
        }
        EventPayload::PermissionGranted { request_id, tool }
        | EventPayload::PermissionDenied { request_id, tool } => {
            event.request_id.clone_from(request_id);
            event.tool.clone_from(tool);
        }
        EventPayload::ShellCommandStarted {
            command,
            cwd,
            tool_call_id,
        } => {
            event.command.clone_from(command);
            event.cwd.clone_from(cwd);
            event.tool_call_id.clone_from(tool_call_id);
        }
        EventPayload::ShellOutputStreamed {
            content,
            tool_call_id,
        } => {
            event.content.clone_from(content);
            event.tool_call_id.clone_from(tool_call_id);
        }
        EventPayload::ShellCommandCompleted {
            command,
            cwd,
            exit_code,
            tool_call_id,
        } => {
            event.command.clone_from(command);
            event.cwd.clone_from(cwd);
            event.exit_code = *exit_code;
            event.tool_call_id.clone_from(tool_call_id);
        }
        EventPayload::FileRevisionUpdated {
            workspace_id,
            target,
        }
        | EventPayload::FileWritten {
            workspace_id,
            target,
        }
        | EventPayload::FileChangedOnDisk {
            workspace_id,
            target,
        } => {
            event.workspace_id.clone_from(workspace_id);
            event.target.clone_from(target);
        }
        EventPayload::SessionInterrupted | EventPayload::SessionCancelled => {}
        EventPayload::PlanUpdated { content }
        | EventPayload::AgentExited { content }
        | EventPayload::ConnectionRestarted { content }
        | EventPayload::SessionResumed { content }
        | EventPayload::ModelChanged { content } => {
            event.content.clone_from(content);
        }
        EventPayload::DeviceRevocationPending {
            execute_at,
            device_name,
            content,
        } => {
            event.execute_at = *execute_at;
            event.device_name.clone_from(device_name);
            event.content.clone_from(content);
        }
        EventPayload::DeviceRevocationCancelled { device_name }
        | EventPayload::DeviceRevocationExecuted { device_name } => {
            event.device_name.clone_from(device_name);
        }
        EventPayload::WorkspaceRegistrationPending { execute_at, target } => {
            event.execute_at = *execute_at;
            event.target.clone_from(target);
        }
        EventPayload::WorkspaceRegistrationCancelled { target }
        | EventPayload::WorkspaceRegistrationExecuted { target } => {
            event.target.clone_from(target);
        }
    }
}

/// Best-effort conversion from a flat wire event back to a typed internal event.
///
/// Used when replaying stored flat events into typed handlers. Fields the flat
/// form carries that the payload variant does not use are ignored.
// Single linear payload adapter — splitting would obscure the variant mapping.
#[allow(clippy::too_many_lines)]
#[must_use]
pub fn wire_to_typed_event(event: &Event) -> TypedEvent {
    let meta = EventMeta {
        id: event.id,
        session_id: event.session_id.clone(),
        timestamp: event.timestamp,
    };
    let payload = match event.event_type {
        EventType::PromptSubmitted => EventPayload::PromptSubmitted {
            role: event.role.clone(),
            content: event.content.clone(),
            attachments: event.attachments.clone(),
            injected_context: event.injected_context.clone(),
        },
        EventType::ResponseStarted => EventPayload::ResponseStarted {
            role: event.role.clone(),
        },
        EventType::StreamUpdate => EventPayload::StreamUpdate {
            role: event.role.clone(),
            content: event.content.clone(),
            streaming: event.streaming,
            thought: event.thought,
            stop_reason: event.stop_reason.clone(),
        },
        EventType::ToolStarted => EventPayload::ToolStarted {
            tool: event.tool.clone(),
            tool_kind: event.tool_kind.clone(),
            tool_call_id: event.tool_call_id.clone(),
            target: event.target.clone(),
            command: event.command.clone(),
            summary: event.summary.clone(),
        },
        EventType::ToolCompleted => EventPayload::ToolCompleted {
            tool: event.tool.clone(),
            tool_kind: event.tool_kind.clone(),
            tool_call_id: event.tool_call_id.clone(),
            target: event.target.clone(),
            summary: event.summary.clone(),
            content: event.content.clone(),
            exit_code: event.exit_code,
        },
        EventType::PlanUpdated => EventPayload::PlanUpdated {
            content: event.content.clone(),
        },
        EventType::PermissionRequested => EventPayload::PermissionRequested {
            request_id: event.request_id.clone(),
            tool: event.tool.clone(),
            tool_kind: event.tool_kind.clone(),
            target: event.target.clone(),
            command: event.command.clone(),
            options: event.options.clone(),
        },
        EventType::PermissionGranted => EventPayload::PermissionGranted {
            request_id: event.request_id.clone(),
            tool: event.tool.clone(),
        },
        EventType::PermissionDenied => EventPayload::PermissionDenied {
            request_id: event.request_id.clone(),
            tool: event.tool.clone(),
        },
        EventType::ShellCommandStarted => EventPayload::ShellCommandStarted {
            command: event.command.clone(),
            cwd: event.cwd.clone(),
            tool_call_id: event.tool_call_id.clone(),
        },
        EventType::ShellOutputStreamed => EventPayload::ShellOutputStreamed {
            content: event.content.clone(),
            tool_call_id: event.tool_call_id.clone(),
        },
        EventType::ShellCommandCompleted => EventPayload::ShellCommandCompleted {
            command: event.command.clone(),
            cwd: event.cwd.clone(),
            exit_code: event.exit_code,
            tool_call_id: event.tool_call_id.clone(),
        },
        EventType::FileRevisionUpdated => EventPayload::FileRevisionUpdated {
            workspace_id: event.workspace_id.clone(),
            target: event.target.clone(),
        },
        EventType::FileWritten => EventPayload::FileWritten {
            workspace_id: event.workspace_id.clone(),
            target: event.target.clone(),
        },
        EventType::FileChangedOnDisk => EventPayload::FileChangedOnDisk {
            workspace_id: event.workspace_id.clone(),
            target: event.target.clone(),
        },
        EventType::SessionInterrupted => EventPayload::SessionInterrupted,
        EventType::SessionCancelled => EventPayload::SessionCancelled,
        EventType::AgentExited => EventPayload::AgentExited {
            content: event.content.clone(),
        },
        EventType::ConnectionRestarted => EventPayload::ConnectionRestarted {
            content: event.content.clone(),
        },
        EventType::SessionResumed => EventPayload::SessionResumed {
            content: event.content.clone(),
        },
        EventType::ModelChanged => EventPayload::ModelChanged {
            content: event.content.clone(),
        },
        EventType::DeviceRevocationPending => EventPayload::DeviceRevocationPending {
            execute_at: event.execute_at,
            device_name: event.device_name.clone(),
            content: event.content.clone(),
        },
        EventType::DeviceRevocationCancelled => EventPayload::DeviceRevocationCancelled {
            device_name: event.device_name.clone(),
        },
        EventType::DeviceRevocationExecuted => EventPayload::DeviceRevocationExecuted {
            device_name: event.device_name.clone(),
        },
        EventType::WorkspaceRegistrationPending => EventPayload::WorkspaceRegistrationPending {
            execute_at: event.execute_at,
            target: event.target.clone(),
        },
        EventType::WorkspaceRegistrationCancelled => EventPayload::WorkspaceRegistrationCancelled {
            target: event.target.clone(),
        },
        EventType::WorkspaceRegistrationExecuted => EventPayload::WorkspaceRegistrationExecuted {
            target: event.target.clone(),
        },
    };
    TypedEvent { meta, payload }
}

/// Serialize a flat wire event to pretty-printed JSON matching Go
/// `json.MarshalIndent` style (2-space indent). Used by golden DTO tests and
/// REST projections.
///
/// # Errors
/// Returns a serde JSON error when serialization fails.
pub fn event_to_json_pretty(event: &Event) -> Result<String, serde_json::Error> {
    serde_json::to_string_pretty(event)
}

/// Build a `TypedEvent` from metadata + payload (helper for producers/tests).
pub fn typed_event(
    id: i64,
    session_id: impl Into<String>,
    timestamp: DateTime<Utc>,
    payload: EventPayload,
) -> TypedEvent {
    TypedEvent {
        meta: EventMeta {
            id,
            session_id: session_id.into(),
            timestamp,
        },
        payload,
    }
}
