// Package interfaces defines the shared contracts between internal packages.
// These interfaces are the boundaries between lanes — each subagent implements
// its own package against these contracts, never implementing another lane's code.
//
// Blueprint references: Sec 3 (Architecture), Sec 11 (Event System),
// Sec 6 (ACP Client Layer), Sec 8 (Permission Manager), Sec 13 (Workspace).
package interfaces

import (
	"context"
	"time"

	"github.com/adama/local-agent/internal/search"
)

// ----------------------------------------------------------------------------
// Event System (Blueprint Sec 11)
// ----------------------------------------------------------------------------

// EventType enumerates all event types in the immutable event log.
type EventType string

const (
	//nolint:revive // event type enum — names are self-documenting
	EventPromptSubmitted       EventType = "PromptSubmitted"
	EventResponseStarted       EventType = "ResponseStarted"
	EventStreamUpdate          EventType = "StreamUpdate"
	EventToolCompleted         EventType = "ToolCompleted"
	EventToolStarted           EventType = "ToolStarted"
	EventPlanUpdated           EventType = "PlanUpdated"
	EventPermissionRequested   EventType = "PermissionRequested"
	EventPermissionGranted     EventType = "PermissionGranted"
	EventPermissionDenied      EventType = "PermissionDenied"
	EventShellCommandStarted   EventType = "ShellCommandStarted"
	EventShellOutputStreamed   EventType = "ShellOutputStreamed"
	EventShellCommandCompleted EventType = "ShellCommandCompleted"
	EventFileRevisionUpdated   EventType = "FileRevisionUpdated"
	// EventFileWritten is broadcast when an agent writes/creates a file via the
	// ACP WriteTextFile callback. It carries the workspace ID (WorkspaceID) and
	// the affected file path (Target) so the frontend can refresh its file tree
	// without a manual reload.
	EventFileWritten EventType = "FileWritten"
	// EventFileChangedOnDisk is broadcast by the filesystem watcher when a file
	// inside a registered workspace changes on disk from OUTSIDE the app (e.g.
	// edited in another editor). Like EventFileWritten it carries WorkspaceID +
	// Target (relative path) so the editor can refresh an open file without a
	// forced reload. Writes performed by the app itself are suppressed (agent
	// writes already emit EventFileWritten), so this fires only for external
	// changes.
	EventFileChangedOnDisk   EventType = "FileChangedOnDisk"
	EventSessionInterrupted  EventType = "SessionInterrupted"
	EventSessionCancelled    EventType = "SessionCancelled"
	EventAgentExited         EventType = "AgentExited"
	EventConnectionRestarted EventType = "ConnectionRestarted"
	EventSessionResumed      EventType = "SessionResumed"
	// EventModelChanged is emitted when the model is switched on a live
	// session via ACP session/set_config_option (no restart, history
	// preserved). Distinct from ConnectionRestarted, which implies the
	// conversation history was reset/exported.
	EventModelChanged EventType = "ModelChanged"
)

// Event is a single entry in the append-only event log.
type Event struct {
	ID         int64     `json:"id"`
	Type       EventType `json:"type"`
	SessionID  string    `json:"sessionId"`
	Timestamp  time.Time `json:"timestamp"`
	Role       string    `json:"role,omitempty"`       // "user" | "agent"
	Content    string    `json:"content,omitempty"`    // message text
	Streaming  bool      `json:"streaming,omitempty"`  // true during streaming
	Tool       string    `json:"tool,omitempty"`       // tool name
	Target     string    `json:"target,omitempty"`     // file path or target
	Summary    string    `json:"summary,omitempty"`    // tool result summary
	Command    string    `json:"command,omitempty"`    // shell command
	Cwd        string    `json:"cwd,omitempty"`        // resolved working directory for shell commands
	Options    []string  `json:"options,omitempty"`    // permission options
	RequestID  string    `json:"requestId,omitempty"`  // permission request ID (for respond)
	ToolKind   string    `json:"toolKind,omitempty"`   // ACP tool kind (read/edit/execute/...)
	ToolCallID string    `json:"toolCallId,omitempty"` // ACP tool call ID (for correlation)
	Thought    bool      `json:"thought,omitempty"`    // true if this is an agent thought chunk
	ExitCode   *int      `json:"exitCode,omitempty"`   // shell/terminal exit code when finished
	// StopReason carries the ACP stop reason for the final StreamUpdate of a
	// turn (e.g. "end_turn", "tool_use", "max_tokens", "refusal", "cancelled").
	// Empty on intermediate streaming chunks. The frontend uses it to surface
	// non-normal terminations (e.g. "hit token limit") subtly under the message.
	StopReason string `json:"stopReason,omitempty"`
	// WorkspaceID identifies the workspace a file-change event applies to (e.g.
	// EventFileWritten), so the frontend can refresh the correct file tree.
	WorkspaceID string `json:"workspaceId,omitempty"`
	// Attachments carries metadata for files attached to a prompt (e.g. uploaded
	// images). Only references are stored — never the blob data — so the event
	// log stays lightweight. The frontend renders thumbnails from the URI; the
	// agent reads the file from disk via Path.
	Attachments []Attachment `json:"attachments,omitempty"`
}

// Attachment describes a file attached to a user prompt (e.g. an uploaded
// image). It carries only references — the blob lives on disk in the uploads
// store — so it is cheap to persist in the event log and broadcast over
// WebSocket.
type Attachment struct {
	// ID is the opaque uploads-store ID, used to build the serving URL.
	ID string `json:"id"`
	// Name is the user-supplied display name of the original file.
	Name string `json:"name"`
	// MimeType is the validated MIME type (e.g. "image/png").
	MimeType string `json:"mimeType"`
	// URI is a file:// URI pointing at the on-disk file, sent to the agent via
	// ACP ImageBlock.Uri or ResourceLinkBlock.Uri. Backend-only: not serialized
	// to JSON (the frontend builds serving URLs from the upload ID, not this).
	URI string `json:"-"`
	// Path is the absolute on-disk path, included in the text fallback so the
	// agent can read the file directly. Persisted to SQLite so it survives
	// reload, but omitted from JSON when empty (the frontend ignores it
	// regardless — it builds serving URLs from the upload ID).
	Path string `json:"path,omitempty"`
}

// EventStore is the contract for the event persistence layer.
// Implemented by the `events` package.
type EventStore interface {
	// Append adds an event to the log. Returns the event with its assigned ID.
	Append(ctx context.Context, e Event) (Event, error)

	// Query retrieves events for a session, optionally filtered by cursor
	// (last event ID seen by the client) for reconnection sync.
	Query(ctx context.Context, sessionID string, afterID int64, limit int) ([]Event, error)

	// QueryAll retrieves events across all sessions, for initial load.
	QueryAll(ctx context.Context, afterID int64, limit int) ([]Event, error)
}

// ----------------------------------------------------------------------------
// Workspace Management (Blueprint Sec 13)
// ----------------------------------------------------------------------------

// FileNode represents a single node in the workspace file tree.
type FileNode struct {
	Name     string     `json:"name"`
	Type     string     `json:"type"` // "folder" | "file"`
	Path     string     `json:"path"`
	Children []FileNode `json:"children,omitempty"`
}

// FileNode type values. These are part of the JSON wire format sent to the UI,
// so the string literals must remain stable.
const (
	FileNodeTypeFile   = "file"
	FileNodeTypeFolder = "folder"
)

// WorkspaceInfo describes a registered workspace.
type WorkspaceInfo struct {
	ID   string `json:"id"`
	Path string `json:"path"`
	Name string `json:"name"`
}

// WorkspaceManager is the contract for workspace operations.
// Implemented by the `workspace` package.
type WorkspaceManager interface {
	// Register adds a directory as a workspace.
	Register(ctx context.Context, path string) (WorkspaceInfo, error)

	// List returns all registered workspaces.
	List(ctx context.Context) ([]WorkspaceInfo, error)

	// Remove deletes a workspace from the in-memory registry by ID.
	Remove(ctx context.Context, id string) error

	// FileTree returns the file tree for a workspace.
	FileTree(ctx context.Context, workspaceID string) ([]FileNode, error)

	// ReadFile returns the content of a file and its current revision.
	ReadFile(ctx context.Context, workspaceID, relPath string) (content string, revision int64, err error)

	// Search runs a workspace-wide content search and returns matching lines.
	Search(ctx context.Context, workspaceID, pattern string, opts search.Options) ([]search.Result, error)
}

// ----------------------------------------------------------------------------
// ACP Client Layer (Blueprint Sec 6, 7, 9, 10)
// ----------------------------------------------------------------------------

// AgentInfo describes a registered agent.
type AgentInfo struct {
	ID      string       `json:"id"`
	Name    string       `json:"name"`
	Command string       `json:"command"`
	Args    []string     `json:"args,omitempty"`
	Models  []AgentModel `json:"models"`
	Warning string       `json:"warning,omitempty"`
}

// AgentModel describes a model offered by an agent.
type AgentModel struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// SessionInfo describes a chat session. It is the interface-layer projection of
// the concrete acp.Session, carrying only the fields the server/UI need so the
// server package never depends on the acp package's concrete types.
type SessionInfo struct {
	ID        string    `json:"id"`
	Name      string    `json:"name"`
	Status    string    `json:"status"`
	AgentID   string    `json:"agentId"`
	ModelID   string    `json:"modelId"`
	Workspace string    `json:"workspace,omitempty"`
	CreatedAt time.Time `json:"createdAt,omitempty"`
	UpdatedAt time.Time `json:"updatedAt,omitempty"`
}

// Session is an alias for SessionInfo, the interface-layer projection of a chat
// session. ListSessions returns a slice of Sessions.
type Session = SessionInfo

// ACPCallbacks allows the ACP client to notify the daemon of events.
// The daemon implements these to persist events and broadcast to clients.
type ACPCallbacks interface {
	OnEvent(event Event)
}

// ACPClient is the contract for communicating with AI agents.
// Implemented by the `acp` package.
type ACPClient interface {
	// ListAgents returns registered agent harnesses and their models.
	ListAgents(ctx context.Context) ([]AgentInfo, error)

	// RegisterAgent adds an agent to the registry.
	RegisterAgent(agent AgentInfo)

	// RemoveAgent removes an agent from the registry.
	RemoveAgent(id string)

	// CreateSession starts a new agent session.
	CreateSession(ctx context.Context, agentID, modelID, workspaceID string) (SessionInfo, error)

	// GetSessionInfo returns metadata for a single session by ID. Returns an
	// error (e.g. "session not found") when no session matches.
	GetSessionInfo(sessionID string) (SessionInfo, error)

	// ListSessions returns all conversations, newest activity first.
	ListSessions() []Session

	// SendPrompt sends a user prompt to the agent and streams responses.
	// attachments carries metadata for files attached to the prompt (e.g.
	// uploaded images); they are translated to ACP content blocks by the
	// transport based on the agent's advertised prompt capabilities.
	// Responses arrive via ACPCallbacks.OnEvent.
	SendPrompt(ctx context.Context, sessionID, content string, attachments []Attachment) error

	// RenameSession changes a conversation's display name.
	RenameSession(sessionID, name string) error

	// RebindSession switches a conversation to a different agent and/or model
	// while preserving its id and event history.
	RebindSession(ctx context.Context, sessionID, agentID, modelID string, maxTransferBytes int) (SessionInfo, error)

	// SwitchModel changes the model on a live session without restarting the
	// agent process. Uses ACP's session/set_config_option when available;
	// falls back to RebindSession for agents that don't support it.
	SwitchModel(ctx context.Context, sessionID, modelID string) error

	// CancelSession interrupts a running session.
	CancelSession(ctx context.Context, sessionID string) error

	// CloseSession closes a session.
	CloseSession(ctx context.Context, sessionID string) error
}

// ----------------------------------------------------------------------------
// Permission Manager (Blueprint Sec 8)
// ----------------------------------------------------------------------------

// PermissionDecision enumerates possible responses to a permission request.
type PermissionDecision string

const (
	//nolint:revive // permission decision enum — names are self-documenting
	PermissionAllowOnce    PermissionDecision = "allow_once"
	PermissionAllowSession PermissionDecision = "allow_session"
	PermissionAllowAlways  PermissionDecision = "allow_always"
	PermissionDeny         PermissionDecision = "deny"
)

// PermissionOptionInfo describes a single selectable permission option as
// offered by the agent. ID is the value echoed back when responding; Name is
// the human-readable label; Kind is the ACP option kind (allow_once,
// reject_once, etc.) used by the UI to pick styling.
type PermissionOptionInfo struct {
	ID   string `json:"id"`
	Name string `json:"name"`
	Kind string `json:"kind"`
}

// PermissionRequest represents a pending permission prompt.
type PermissionRequest struct {
	ID            string                 `json:"id"`
	SessionID     string                 `json:"sessionId"`
	Tool          string                 `json:"tool"`
	ToolKind      string                 `json:"toolKind,omitempty"`
	Command       string                 `json:"command,omitempty"`
	Target        string                 `json:"target,omitempty"`
	Options       []PermissionDecision   `json:"options"`
	OptionDetails []PermissionOptionInfo `json:"optionDetails,omitempty"`
}

// PermissionManager is the contract for permission handling.
// Implemented by the `permissions` package.
type PermissionManager interface {
	// Request broadcasts a permission prompt to all paired devices.
	// Blocks until a decision is received or the context is cancelled.
	Request(ctx context.Context, req PermissionRequest) (PermissionDecision, error)

	// Respond records a decision from a device. First response wins.
	Respond(ctx context.Context, requestID string, decision PermissionDecision) error

	// ClearSession drops all cached permission policies for the given session.
	// Called when a session closes so allow_always/allow_session decisions do
	// not leak across session lifetimes.
	ClearSession(sessionID string)
}

// ----------------------------------------------------------------------------
// File Sync (Blueprint Sec 14)
// ----------------------------------------------------------------------------

// FileSync is the contract for file revision tracking and merge.
// Implemented by the `files` package.
type FileSync interface {
	// Save writes file content with optimistic locking via expectedRevision.
	// Returns ErrStaleRevision if the file has been modified since.
	Save(ctx context.Context, workspaceID, relPath, content string, expectedRevision int64) (newRevision int64, err error)

	// CurrentRevision returns the latest revision of a file.
	CurrentRevision(ctx context.Context, workspaceID, relPath string) (int64, error)
}
