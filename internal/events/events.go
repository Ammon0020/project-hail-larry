// Package events implements the event system with SQLite persistence.
// Blueprint references: Sec 11 (Event System).
//
// Events are appended chronologically to an immutable log. Application state
// is derived from event history, simplifying multi-client sync and replay.
// The store uses SQLite in WAL mode for concurrent readers with append-heavy writes.
package events

import (
	"context"
	"database/sql"
	"encoding/json"
	"fmt"
	"time"

	"github.com/adama/local-agent/internal/interfaces"

	_ "modernc.org/sqlite" // pure-Go SQLite driver (no CGO required)
)

// Store implements interfaces.EventStore using SQLite.
type Store struct {
	db *sql.DB
}

// sqliteBusyTimeout is how long a contended SQLite write waits before failing
// with SQLITE_BUSY. Set per-connection so concurrent pool connections all
// honor it.
const sqliteBusyTimeout = 5000 // milliseconds

// New creates a new event Store, initializing the SQLite database at dbPath.
// The database is opened with WAL mode for concurrent read/write access.
//
// SQLite serializes writes through a single file lock. The default
// database/sql pool opens many connections, and without a busy_timeout a
// contended write returns SQLITE_BUSY immediately ("database is locked"). To
// handle concurrent appends we constrain the pool and set a busy_timeout so
// contended writers wait briefly instead of failing. busy_timeout is
// connection-scoped, so with MaxOpenConns(1) every statement runs on the same
// connection and the PRAGMA persists for its lifetime.
func New(dbPath string) (*Store, error) {
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}

	// Constrain the connection pool. A single connection serializes all DB
	// access (simplest correct option for SQLite), eliminating lock contention
	// between pooled connections.
	db.SetMaxOpenConns(1)
	db.SetMaxIdleConns(1)
	db.SetConnMaxLifetime(0)

	// Enable WAL mode for append-heavy workloads with concurrent readers.
	if _, err := db.Exec("PRAGMA journal_mode=WAL"); err != nil {
		_ = db.Close()
		return nil, fmt.Errorf("set WAL mode: %w", err)
	}

	// Set busy_timeout so a contended write waits up to sqliteBusyTimeout ms
	// instead of failing immediately. With MaxOpenConns(1) this runs on the
	// single pooled connection and persists for its lifetime.
	if _, err := db.Exec(fmt.Sprintf("PRAGMA busy_timeout=%d", sqliteBusyTimeout)); err != nil {
		_ = db.Close()
		return nil, fmt.Errorf("set busy_timeout: %w", err)
	}

	// Create the events table if it doesn't exist.
	schema := `
	CREATE TABLE IF NOT EXISTS events (
		id INTEGER PRIMARY KEY AUTOINCREMENT,
		type TEXT NOT NULL,
		session_id TEXT NOT NULL,
		timestamp DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
		payload TEXT NOT NULL
	);
	CREATE INDEX IF NOT EXISTS idx_events_session_id ON events(session_id);
	CREATE INDEX IF NOT EXISTS idx_events_session_id_id ON events(session_id, id);
	`
	if _, err := db.Exec(schema); err != nil {
		_ = db.Close()
		return nil, fmt.Errorf("create schema: %w", err)
	}

	return &Store{db: db}, nil
}

// Close closes the underlying database connection.
func (s *Store) Close() error {
	return s.db.Close()
}

// Append adds an event to the log. Returns the event with its assigned ID and timestamp.
func (s *Store) Append(ctx context.Context, e interfaces.Event) (interfaces.Event, error) {
	if e.Timestamp.IsZero() {
		e.Timestamp = time.Now().UTC()
	}

	payload, err := json.Marshal(eventPayload{
		Role:        e.Role,
		Content:     e.Content,
		Streaming:   e.Streaming,
		Tool:        e.Tool,
		Target:      e.Target,
		Summary:     e.Summary,
		Command:     e.Command,
		Cwd:         e.Cwd,
		Options:     e.Options,
		RequestID:   e.RequestID,
		ToolKind:    e.ToolKind,
		ToolCallID:  e.ToolCallID,
		Thought:     e.Thought,
		ExitCode:    e.ExitCode,
		WorkspaceID: e.WorkspaceID,
		Attachments: e.Attachments,
	})
	if err != nil {
		return e, fmt.Errorf("marshal payload: %w", err)
	}

	result, err := s.db.ExecContext(ctx,
		"INSERT INTO events (type, session_id, timestamp, payload) VALUES (?, ?, ?, ?)",
		string(e.Type), e.SessionID, e.Timestamp, string(payload),
	)
	if err != nil {
		return e, fmt.Errorf("insert event: %w", err)
	}

	id, err := result.LastInsertId()
	if err != nil {
		return e, fmt.Errorf("get last insert id: %w", err)
	}

	e.ID = id
	return e, nil
}

// Query retrieves events for a session, optionally filtered by cursor
// (last event ID seen by the client) for reconnection sync.
func (s *Store) Query(ctx context.Context, sessionID string, afterID int64, limit int) ([]interfaces.Event, error) {
	if limit <= 0 {
		limit = 1000
	}

	rows, err := s.db.QueryContext(ctx,
		"SELECT id, type, session_id, timestamp, payload FROM events WHERE session_id = ? AND id > ? ORDER BY id ASC LIMIT ?",
		sessionID, afterID, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("query events: %w", err)
	}
	defer func() { _ = rows.Close() }()

	return scanEvents(rows)
}

// QueryAll retrieves events across all sessions, for initial load.
func (s *Store) QueryAll(ctx context.Context, afterID int64, limit int) ([]interfaces.Event, error) {
	if limit <= 0 {
		limit = 1000
	}

	rows, err := s.db.QueryContext(ctx,
		"SELECT id, type, session_id, timestamp, payload FROM events WHERE id > ? ORDER BY id ASC LIMIT ?",
		afterID, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("query all events: %w", err)
	}
	defer func() { _ = rows.Close() }()

	return scanEvents(rows)
}

// eventPayload holds the variable fields of an event, stored as JSON in the payload column.
type eventPayload struct {
	Role        string                  `json:"role,omitempty"`
	Content     string                  `json:"content,omitempty"`
	Streaming   bool                    `json:"streaming,omitempty"`
	Tool        string                  `json:"tool,omitempty"`
	Target      string                  `json:"target,omitempty"`
	Summary     string                  `json:"summary,omitempty"`
	Command     string                  `json:"command,omitempty"`
	Cwd         string                  `json:"cwd,omitempty"`
	Options     []string                `json:"options,omitempty"`
	RequestID   string                  `json:"requestId,omitempty"`
	ToolKind    string                  `json:"toolKind,omitempty"`
	ToolCallID  string                  `json:"toolCallId,omitempty"`
	Thought     bool                    `json:"thought,omitempty"`
	ExitCode    *int                    `json:"exitCode,omitempty"`
	WorkspaceID string                  `json:"workspaceId,omitempty"`
	Attachments []interfaces.Attachment `json:"attachments,omitempty"`
}

// scanEvents converts sql.Rows into a slice of Event structs.
func scanEvents(rows *sql.Rows) ([]interfaces.Event, error) {
	var events []interfaces.Event

	for rows.Next() {
		var e interfaces.Event
		var eType string
		var timestampStr string
		var payloadStr string

		if err := rows.Scan(&e.ID, &eType, &e.SessionID, &timestampStr, &payloadStr); err != nil {
			return nil, fmt.Errorf("scan event: %w", err)
		}

		e.Type = interfaces.EventType(eType)

		// Parse timestamp — try common SQLite formats. An unparseable
		// timestamp means the row is corrupted; surface the error instead of
		// silently substituting time.Now(), which would corrupt the event
		// history's chronological ordering and replay correctness.
		ts, err := time.Parse("2006-01-02 15:04:05", timestampStr)
		if err != nil {
			ts, err = time.Parse(time.RFC3339, timestampStr)
			if err != nil {
				return nil, fmt.Errorf("parse timestamp %q: %w", timestampStr, err)
			}
		}
		e.Timestamp = ts

		// Unmarshal payload.
		var payload eventPayload
		if err := json.Unmarshal([]byte(payloadStr), &payload); err != nil {
			return nil, fmt.Errorf("unmarshal payload: %w", err)
		}

		e.Role = payload.Role
		e.Content = payload.Content
		e.Streaming = payload.Streaming
		e.Tool = payload.Tool
		e.Target = payload.Target
		e.Summary = payload.Summary
		e.Command = payload.Command
		e.Options = payload.Options
		e.RequestID = payload.RequestID
		e.ToolKind = payload.ToolKind
		e.ToolCallID = payload.ToolCallID
		e.Thought = payload.Thought
		e.ExitCode = payload.ExitCode
		e.Cwd = payload.Cwd
		e.WorkspaceID = payload.WorkspaceID
		e.Attachments = payload.Attachments

		events = append(events, e)
	}

	return events, rows.Err()
}
