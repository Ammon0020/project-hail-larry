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
	EventSessionInterrupted    EventType = "SessionInterrupted"
	EventSessionCancelled      EventType = "SessionCancelled"
	EventAgentExited           EventType = "AgentExited"
	EventConnectionRestarted   EventType = "ConnectionRestarted"
	EventSessionResumed        EventType = "SessionResumed"
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
	Options    []string  `json:"options,omitempty"`    // permission options
	RequestID  string    `json:"requestId,omitempty"`  // permission request ID (for respond)
	ToolKind   string    `json:"toolKind,omitempty"`   // ACP tool kind (read/edit/execute/...)
	ToolCallID string    `json:"toolCallId,omitempty"` // ACP tool call ID (for correlation)
	Thought    bool      `json:"thought,omitempty"`    // true if this is an agent thought chunk
	ExitCode   *int      `json:"exitCode,omitempty"`   // shell/terminal exit code when finished
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
}

// ----------------------------------------------------------------------------
// ACP Client Layer (Blueprint Sec 6, 7, 9, 10)
// ----------------------------------------------------------------------------

// AgentInfo describes a registered agent.
type AgentInfo struct {
	ID      string       `json:"id"`
	Name    string       `json:"name"`
	Models  []AgentModel `json:"models"`
	Warning string       `json:"warning,omitempty"`
}

// AgentModel describes a model offered by an agent.
type AgentModel struct {
	ID   string `json:"id"`
	Name string `json:"name"`
}

// SessionInfo describes a chat session.
type SessionInfo struct {
	ID     string `json:"id"`
	Name   string `json:"name"`
	Status string `json:"status"`
}

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

	// CreateSession starts a new agent session.
	CreateSession(ctx context.Context, agentID, modelID, workspaceID string) (SessionInfo, error)

	// SendPrompt sends a user prompt to the agent and streams responses.
	// Responses arrive via ACPCallbacks.OnEvent.
	SendPrompt(ctx context.Context, sessionID, content string) error

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
