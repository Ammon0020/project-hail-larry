//! Shared DTOs for cross-module contracts (Go `internal/interfaces` + search DTOs).
//!
//! This module is intentionally dependency-free of service implementations
//! (`search`, `events`, `workspace`, …). Search options/results live here so
//! the trait layer never depends on the search crate implementation.
//!
//! Wire shapes are frozen by S-CONTRACT (`tests/contract/golden/dto/`). Serde
//! renames / `skip_serializing_if` mirror Go `json` tags exactly so golden
//! fixtures pass. Time fields use `chrono::DateTime<Utc>` with RFC3339 wire
//! format (Go `time.Time`).
//!
//! Agent DTOs (`AgentInfo`, `AgentModel`) already live in [`crate::config`] and
//! are re-exported below — do not duplicate them.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// Re-export agent DTOs from config so callers can import them from interfaces.
pub use crate::config::{AgentInfo, AgentModel};

// ============================================================================
// Event system (Blueprint Sec 11)
// ============================================================================

/// Event type enum — 27 variants matching Go `interfaces.EventType` string
/// constants exactly. The wire form is a bare string (`"PromptSubmitted"`, …);
/// serde renames keep the Rust variants idiomatic while preserving Go JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum EventType {
    #[serde(rename = "PromptSubmitted")]
    PromptSubmitted,
    #[serde(rename = "ResponseStarted")]
    ResponseStarted,
    #[serde(rename = "StreamUpdate")]
    StreamUpdate,
    #[serde(rename = "ToolCompleted")]
    ToolCompleted,
    #[serde(rename = "ToolStarted")]
    ToolStarted,
    #[serde(rename = "PlanUpdated")]
    PlanUpdated,
    #[serde(rename = "PermissionRequested")]
    PermissionRequested,
    #[serde(rename = "PermissionGranted")]
    PermissionGranted,
    #[serde(rename = "PermissionDenied")]
    PermissionDenied,
    #[serde(rename = "ShellCommandStarted")]
    ShellCommandStarted,
    #[serde(rename = "ShellOutputStreamed")]
    ShellOutputStreamed,
    #[serde(rename = "ShellCommandCompleted")]
    ShellCommandCompleted,
    #[serde(rename = "FileRevisionUpdated")]
    FileRevisionUpdated,
    /// Agent wrote/created a file via ACP `WriteTextFile`.
    #[serde(rename = "FileWritten")]
    FileWritten,
    /// Filesystem watcher: external on-disk change (not app/agent write).
    #[serde(rename = "FileChangedOnDisk")]
    FileChangedOnDisk,
    #[serde(rename = "SessionInterrupted")]
    SessionInterrupted,
    #[serde(rename = "SessionCancelled")]
    SessionCancelled,
    #[serde(rename = "AgentExited")]
    AgentExited,
    #[serde(rename = "ConnectionRestarted")]
    ConnectionRestarted,
    #[serde(rename = "SessionResumed")]
    SessionResumed,
    /// Model switched on a live session (history preserved).
    #[serde(rename = "ModelChanged")]
    ModelChanged,
    #[serde(rename = "DeviceRevocationPending")]
    DeviceRevocationPending,
    #[serde(rename = "DeviceRevocationCancelled")]
    DeviceRevocationCancelled,
    #[serde(rename = "DeviceRevocationExecuted")]
    DeviceRevocationExecuted,
    #[serde(rename = "WorkspaceRegistrationPending")]
    WorkspaceRegistrationPending,
    #[serde(rename = "WorkspaceRegistrationCancelled")]
    WorkspaceRegistrationCancelled,
    #[serde(rename = "WorkspaceRegistrationExecuted")]
    WorkspaceRegistrationExecuted,
}

impl EventType {
    /// Stable string form matching the Go wire constants.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PromptSubmitted => "PromptSubmitted",
            Self::ResponseStarted => "ResponseStarted",
            Self::StreamUpdate => "StreamUpdate",
            Self::ToolCompleted => "ToolCompleted",
            Self::ToolStarted => "ToolStarted",
            Self::PlanUpdated => "PlanUpdated",
            Self::PermissionRequested => "PermissionRequested",
            Self::PermissionGranted => "PermissionGranted",
            Self::PermissionDenied => "PermissionDenied",
            Self::ShellCommandStarted => "ShellCommandStarted",
            Self::ShellOutputStreamed => "ShellOutputStreamed",
            Self::ShellCommandCompleted => "ShellCommandCompleted",
            Self::FileRevisionUpdated => "FileRevisionUpdated",
            Self::FileWritten => "FileWritten",
            Self::FileChangedOnDisk => "FileChangedOnDisk",
            Self::SessionInterrupted => "SessionInterrupted",
            Self::SessionCancelled => "SessionCancelled",
            Self::AgentExited => "AgentExited",
            Self::ConnectionRestarted => "ConnectionRestarted",
            Self::SessionResumed => "SessionResumed",
            Self::ModelChanged => "ModelChanged",
            Self::DeviceRevocationPending => "DeviceRevocationPending",
            Self::DeviceRevocationCancelled => "DeviceRevocationCancelled",
            Self::DeviceRevocationExecuted => "DeviceRevocationExecuted",
            Self::WorkspaceRegistrationPending => "WorkspaceRegistrationPending",
            Self::WorkspaceRegistrationCancelled => "WorkspaceRegistrationCancelled",
            Self::WorkspaceRegistrationExecuted => "WorkspaceRegistrationExecuted",
        }
    }

    /// All 27 event type variants in declaration order (for exhaustive tests).
    pub fn all() -> &'static [EventType] {
        &ALL_EVENT_TYPES
    }
}

impl std::fmt::Display for EventType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

const ALL_EVENT_TYPES: [EventType; 27] = [
    EventType::PromptSubmitted,
    EventType::ResponseStarted,
    EventType::StreamUpdate,
    EventType::ToolCompleted,
    EventType::ToolStarted,
    EventType::PlanUpdated,
    EventType::PermissionRequested,
    EventType::PermissionGranted,
    EventType::PermissionDenied,
    EventType::ShellCommandStarted,
    EventType::ShellOutputStreamed,
    EventType::ShellCommandCompleted,
    EventType::FileRevisionUpdated,
    EventType::FileWritten,
    EventType::FileChangedOnDisk,
    EventType::SessionInterrupted,
    EventType::SessionCancelled,
    EventType::AgentExited,
    EventType::ConnectionRestarted,
    EventType::SessionResumed,
    EventType::ModelChanged,
    EventType::DeviceRevocationPending,
    EventType::DeviceRevocationCancelled,
    EventType::DeviceRevocationExecuted,
    EventType::WorkspaceRegistrationPending,
    EventType::WorkspaceRegistrationCancelled,
    EventType::WorkspaceRegistrationExecuted,
];

/// Flat event entry matching Go `interfaces.Event`.
///
/// This is the durable/store shape and the intermediate used by the wire
/// adapter when projecting typed internal events to Go-compatible JSON. Optional
/// fields use `skip_serializing_if` to mirror Go `omitempty`. Note: Go's
/// `time.Time` `omitempty` does **not** drop the zero value on the wire (see
/// golden `event_minimal.json` which always includes `executeAt`); we therefore
/// always serialize `execute_at`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: i64,
    #[serde(rename = "type")]
    pub event_type: EventType,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    /// `"user"` | `"agent"`.
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
    /// Resolved working directory for shell commands.
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
    /// ACP stop reason on the final `StreamUpdate` of a turn.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub stop_reason: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace_id: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub attachments: Vec<Attachment>,
    /// Scheduled execution time for grace-period pending actions.
    /// Always serialized (Go emits zero times despite `omitempty` on `time.Time`).
    pub execute_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_name: String,
}

impl Event {
    /// Construct a minimal event with required fields only (empty optionals,
    /// year-1 zero `execute_at` matching Go `time.Time{}`).
    pub fn new(
        id: i64,
        event_type: EventType,
        session_id: impl Into<String>,
        timestamp: DateTime<Utc>,
    ) -> Self {
        Self {
            id,
            event_type,
            session_id: session_id.into(),
            timestamp,
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
            execute_at: go_zero_time(),
            device_name: String::new(),
        }
    }
}

/// File attached to a user prompt. Blob data lives in the uploads store; only
/// references are persisted on the event. Mirrors Go `interfaces.Attachment`.
///
/// `uri` is backend-only (`json:"-"` in Go) → `#[serde(skip)]`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: String,
    pub name: String,
    pub mime_type: String,
    /// file:// URI for ACP ImageBlock/ResourceLink — never sent over JSON.
    #[serde(skip)]
    pub uri: String,
    /// Absolute on-disk path; omitted when empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
}

// ============================================================================
// Typed internal event (not the public JSON contract)
// ============================================================================

/// Common metadata shared by every typed internal event.
///
/// The public wire contract is the flat Go JSON shape produced by
/// [`crate::interfaces::wire`]; this struct is for type-safe internal use.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventMeta {
    pub id: i64,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
}

/// Typed internal event: metadata + payload kind.
///
/// Convert to the flat Go wire form via [`crate::interfaces::wire::typed_event_to_wire`].
#[derive(Debug, Clone, PartialEq)]
pub struct TypedEvent {
    pub meta: EventMeta,
    pub payload: EventPayload,
}

/// Payload discriminant + fields for typed internal events.
///
/// Variants group fields that are meaningful together; sparse Go-side optionals
/// that a variant never sets stay empty on the wire via omitempty.
#[derive(Debug, Clone, PartialEq)]
pub enum EventPayload {
    PromptSubmitted {
        role: String,
        content: String,
        attachments: Vec<Attachment>,
    },
    ResponseStarted {
        role: String,
    },
    StreamUpdate {
        role: String,
        content: String,
        streaming: bool,
        thought: bool,
        stop_reason: String,
    },
    ToolStarted {
        tool: String,
        tool_kind: String,
        tool_call_id: String,
        target: String,
        command: String,
        /// Initial ACP tool status (`pending`, `in_progress`, …). Go exposes
        /// this in the flat event's `summary` field.
        summary: String,
    },
    ToolCompleted {
        tool: String,
        tool_kind: String,
        tool_call_id: String,
        target: String,
        summary: String,
        /// Rendered ACP text/diff/raw output. The flat Go-compatible event
        /// stores this independently from the lifecycle status in `summary`.
        content: String,
        exit_code: Option<i32>,
    },
    PlanUpdated {
        content: String,
    },
    PermissionRequested {
        request_id: String,
        tool: String,
        tool_kind: String,
        target: String,
        command: String,
        options: Vec<String>,
    },
    PermissionGranted {
        request_id: String,
        tool: String,
    },
    PermissionDenied {
        request_id: String,
        tool: String,
    },
    ShellCommandStarted {
        command: String,
        cwd: String,
        tool_call_id: String,
    },
    ShellOutputStreamed {
        content: String,
        tool_call_id: String,
    },
    ShellCommandCompleted {
        command: String,
        cwd: String,
        exit_code: Option<i32>,
        tool_call_id: String,
    },
    FileRevisionUpdated {
        workspace_id: String,
        target: String,
    },
    FileWritten {
        workspace_id: String,
        target: String,
    },
    FileChangedOnDisk {
        workspace_id: String,
        target: String,
    },
    SessionInterrupted,
    SessionCancelled,
    AgentExited {
        content: String,
    },
    ConnectionRestarted {
        content: String,
    },
    SessionResumed {
        content: String,
    },
    ModelChanged {
        content: String,
    },
    DeviceRevocationPending {
        execute_at: DateTime<Utc>,
        device_name: String,
        content: String,
    },
    DeviceRevocationCancelled {
        device_name: String,
    },
    DeviceRevocationExecuted {
        device_name: String,
    },
    WorkspaceRegistrationPending {
        execute_at: DateTime<Utc>,
        target: String,
    },
    WorkspaceRegistrationCancelled {
        target: String,
    },
    WorkspaceRegistrationExecuted {
        target: String,
    },
}

impl EventPayload {
    /// Map payload to the corresponding [`EventType`] wire discriminant.
    pub fn event_type(&self) -> EventType {
        match self {
            Self::PromptSubmitted { .. } => EventType::PromptSubmitted,
            Self::ResponseStarted { .. } => EventType::ResponseStarted,
            Self::StreamUpdate { .. } => EventType::StreamUpdate,
            Self::ToolCompleted { .. } => EventType::ToolCompleted,
            Self::ToolStarted { .. } => EventType::ToolStarted,
            Self::PlanUpdated { .. } => EventType::PlanUpdated,
            Self::PermissionRequested { .. } => EventType::PermissionRequested,
            Self::PermissionGranted { .. } => EventType::PermissionGranted,
            Self::PermissionDenied { .. } => EventType::PermissionDenied,
            Self::ShellCommandStarted { .. } => EventType::ShellCommandStarted,
            Self::ShellOutputStreamed { .. } => EventType::ShellOutputStreamed,
            Self::ShellCommandCompleted { .. } => EventType::ShellCommandCompleted,
            Self::FileRevisionUpdated { .. } => EventType::FileRevisionUpdated,
            Self::FileWritten { .. } => EventType::FileWritten,
            Self::FileChangedOnDisk { .. } => EventType::FileChangedOnDisk,
            Self::SessionInterrupted => EventType::SessionInterrupted,
            Self::SessionCancelled => EventType::SessionCancelled,
            Self::AgentExited { .. } => EventType::AgentExited,
            Self::ConnectionRestarted { .. } => EventType::ConnectionRestarted,
            Self::SessionResumed { .. } => EventType::SessionResumed,
            Self::ModelChanged { .. } => EventType::ModelChanged,
            Self::DeviceRevocationPending { .. } => EventType::DeviceRevocationPending,
            Self::DeviceRevocationCancelled { .. } => EventType::DeviceRevocationCancelled,
            Self::DeviceRevocationExecuted { .. } => EventType::DeviceRevocationExecuted,
            Self::WorkspaceRegistrationPending { .. } => EventType::WorkspaceRegistrationPending,
            Self::WorkspaceRegistrationCancelled { .. } => {
                EventType::WorkspaceRegistrationCancelled
            }
            Self::WorkspaceRegistrationExecuted { .. } => EventType::WorkspaceRegistrationExecuted,
        }
    }
}

// ============================================================================
// Workspace
// ============================================================================

/// File-tree node type values (stable wire strings).
pub const FILE_NODE_TYPE_FILE: &str = "file";
pub const FILE_NODE_TYPE_FOLDER: &str = "folder";

/// Single node in the workspace file tree. Mirrors Go `interfaces.FileNode`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    pub name: String,
    /// `"folder"` | `"file"`.
    #[serde(rename = "type")]
    pub node_type: String,
    pub path: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<FileNode>,
}

/// Registered workspace descriptor. Mirrors Go `interfaces.WorkspaceInfo`.
///
/// `available` / `error` are Rust UX extensions (missing-path warning). Healthy
/// entries omit them on the wire (`skip_serializing_if`) so existing goldens
/// stay compatible.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceInfo {
    pub id: String,
    pub path: String,
    pub name: String,
    /// False when the path failed to load (missing/invalid). Defaults true.
    #[serde(default = "default_true", skip_serializing_if = "is_true")]
    pub available: bool,
    /// Human-readable load failure; empty when available.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub error: String,
}

// ============================================================================
// Session / provider
// ============================================================================

/// Chat session projection for server/UI. Mirrors Go `interfaces.SessionInfo`.
///
/// `Session` is a type alias for `SessionInfo` (Go `type Session = SessionInfo`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub id: String,
    pub name: String,
    pub status: String,
    pub agent_id: String,
    pub model_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub workspace: String,
    #[serde(default, skip_serializing_if = "is_zero_datetime")]
    pub created_at: DateTime<Utc>,
    #[serde(default, skip_serializing_if = "is_zero_datetime")]
    pub updated_at: DateTime<Utc>,
}

/// Negotiated ACP session-history caps from a live `initialize` (S-HIST-PROBE).
///
/// Returned by `GET /api/sessions/{id}/capabilities`. When [`Self::available`]
/// is false the agent process is not warm — do not infer missing list/load
/// (epic Q8 cold-start probe still open).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionHistoryCapabilities {
    /// True when caps were read from a live agent `initialize` response.
    pub available: bool,
    /// `sessionCapabilities.list` advertised.
    pub can_list_sessions: bool,
    /// `agentCapabilities.loadSession` advertised.
    pub can_load_session: bool,
    /// `sessionCapabilities.resume` advertised.
    pub can_resume_session: bool,
    /// `sessionCapabilities.close` advertised (optional lifecycle).
    pub can_close_session: bool,
    /// `sessionCapabilities.delete` advertised (optional lifecycle).
    pub can_delete_session: bool,
}

impl SessionHistoryCapabilities {
    /// Caps for a known session that has no live initialize yet (dormant).
    #[must_use]
    pub const fn unavailable() -> Self {
        Self {
            available: false,
            can_list_sessions: false,
            can_load_session: false,
            can_resume_session: false,
            can_close_session: false,
            can_delete_session: false,
        }
    }
}

/// Alias matching Go `type Session = SessionInfo`.
pub type Session = SessionInfo;

/// Configurable LLM provider advertised by an agent.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderInfo {
    pub id: String,
    pub required: bool,
    pub supported: Vec<String>,
    /// `None` when the provider is disabled (omitempty).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current: Option<ProviderCurrentConfig>,
}

/// Non-secret effective routing config for a provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProviderCurrentConfig {
    pub api_type: String,
    pub base_url: String,
}

// ============================================================================
// Permissions
// ============================================================================

/// Decision values for permission prompts. Wire strings match Go constants.
///
/// `RejectAlways` mirrors the Go `permissions.PermissionRejectAlways` local
/// constant (`"reject_always"`): a durable deny counterpart to `AllowAlways`,
/// kept on the same enum so it round-trips through the respond API and the
/// policy cache without a parallel string type. It is defined on this enum
/// rather than locally in `permissions` because Rust enums are closed — the
/// respond path validates a `PermissionDecision` against the request's offered
/// options, so the value must inhabit the same type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PermissionDecision {
    #[serde(rename = "allow_once")]
    AllowOnce,
    #[serde(rename = "allow_session")]
    AllowSession,
    #[serde(rename = "allow_always")]
    AllowAlways,
    #[serde(rename = "deny")]
    Deny,
    /// Durable deny: auto-deny subsequent matching requests without re-prompting.
    /// Go counterpart: `permissions.PermissionRejectAlways` (`"reject_always"`).
    #[serde(rename = "reject_always")]
    RejectAlways,
}

impl PermissionDecision {
    /// Stable string form matching the Go wire constants.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AllowOnce => "allow_once",
            Self::AllowSession => "allow_session",
            Self::AllowAlways => "allow_always",
            Self::Deny => "deny",
            Self::RejectAlways => "reject_always",
        }
    }
}

impl std::fmt::Display for PermissionDecision {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Selectable permission option offered by the agent (ACP option details).
///
/// Go name: `PermissionOptionInfo`. Story name `PermissionOption` is an alias.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionOptionInfo {
    pub id: String,
    pub name: String,
    pub kind: String,
}

/// Story alias for [`PermissionOptionInfo`].
pub type PermissionOption = PermissionOptionInfo;

/// Pending permission prompt. Mirrors Go `interfaces.PermissionRequest`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionRequest {
    pub id: String,
    pub session_id: String,
    pub tool: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub tool_kind: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub command: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub target: String,
    pub options: Vec<PermissionDecision>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub option_details: Vec<PermissionOptionInfo>,
}

/// Device response to a pending permission request.
///
/// Go does not define a dedicated `PermissionResponse` struct (the respond API
/// takes a bare `PermissionDecision`); this typed envelope is the Rust-side
/// request/response pairing used by handlers and the permission manager.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PermissionResponse {
    pub request_id: String,
    pub decision: PermissionDecision,
}

// ============================================================================
// Search DTOs (moved out of search so traits never depend on the implementation)
// ============================================================================

/// Controls a workspace content-search run. Mirrors Go `search.Options`.
///
/// Fields intentionally have no serde attributes — search options are request
/// parameters, not a frozen golden DTO. The implementation maps them to rg flags.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SearchOptions {
    /// Regular expression to search for (required).
    pub pattern: String,
    /// Case-insensitive matching when true.
    pub ignore_case: bool,
    /// Cap on returned matches; `<= 0` means the search default (200).
    pub max_results: i32,
    /// Optional glob restricting which file paths are searched (e.g. `"*.go"`).
    pub file_pattern: String,
    /// Context lines before/after each match (rg only; Go fallback reports the match line).
    pub context_lines: i32,
}

/// Single content-search match. Mirrors Go `search.Result`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SearchResult {
    /// Path relative to the workspace root.
    pub path: String,
    /// 1-based line number of the match.
    pub line_number: i32,
    /// Full text of the matched line.
    pub line_content: String,
    /// 0-based byte offset where the match begins within `line_content`.
    pub match_start: i32,
    /// 0-based exclusive end offset within `line_content`.
    pub match_end: i32,
}

// ============================================================================
// Pairing / device DTOs (Go lives in `pairing`; shared here for wire shape + fixtures)
// ============================================================================

/// Pending grace-period action types (stable wire strings).
pub const PENDING_ACTION_TYPE_REVOCATION: &str = "revocation";
pub const PENDING_ACTION_TYPE_WORKSPACE_REGISTRATION: &str = "workspace_registration";

/// Timer-free view of a grace-period pending action for API responses.
/// Mirrors Go `pairing.PendingActionInfo`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PendingActionInfo {
    pub id: String,
    /// `"revocation"` | `"workspace_registration"`.
    #[serde(rename = "type")]
    pub action_type: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_id: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub device_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub path: String,
    pub requested_by: String,
    pub requested_at: DateTime<Utc>,
    pub execute_at: DateTime<Utc>,
}

/// Short-lived single-use pairing session. Mirrors Go `pairing.PairingSession`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PairingSession {
    pub id: String,
    pub token: String,
    pub passcode: String,
    pub url: String,
    pub qr_path: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

impl std::fmt::Debug for PairingSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PairingSession")
            .field("id", &self.id)
            .field("url", &"<redacted>")
            .field("qr_path", &self.qr_path)
            .field("created_at", &self.created_at)
            .field("expires_at", &self.expires_at)
            .field("used", &self.used)
            .finish()
    }
}

/// Long-lived credential returned once at pairing time.
/// Mirrors Go `pairing.DeviceCredential`.
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCredential {
    pub id: String,
    pub name: String,
    pub secret: String,
    pub paired_at: DateTime<Utc>,
}

impl std::fmt::Debug for DeviceCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceCredential")
            .field("id", &self.id)
            .field("name", &self.name)
            .field("paired_at", &self.paired_at)
            .finish()
    }
}

/// Public, secret-free view of a paired device for list/admin APIs.
/// Mirrors Go `pairing.DeviceInfo`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub paired_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
}

// ============================================================================
// Helpers
// ============================================================================

/// RFC3339 / Go zero datetime (`time.Time{}` → `0001-01-01T00:00:00Z`).
///
/// Prefer `Option` + skip when a field is truly optional on the wire; for
/// Go-matching flat events we use this explicit zero instant because Go's
/// `omitempty` on `time.Time` still emits the year-1 value.
pub fn go_zero_time() -> DateTime<Utc> {
    static ZERO_TIME: OnceLock<DateTime<Utc>> = OnceLock::new();
    *ZERO_TIME.get_or_init(|| {
        DateTime::parse_from_rfc3339("0001-01-01T00:00:00Z")
            .map_or_else(|_| DateTime::<Utc>::UNIX_EPOCH, |dt| dt.with_timezone(&Utc))
    })
}

/// `skip_serializing_if` helper: treat year-1 zero times as empty (SessionInfo
/// `createdAt`/`updatedAt` omitempty).
fn is_zero_datetime(dt: &DateTime<Utc>) -> bool {
    *dt == go_zero_time()
}

fn default_true() -> bool {
    true
}

fn is_true(value: &bool) -> bool {
    *value
}
