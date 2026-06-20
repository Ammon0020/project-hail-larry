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

	_ "modernc.org/sqlite"
)

// Store implements interfaces.EventStore using SQLite.
type Store struct {
	db *sql.DB
}

// New creates a new event Store, initializing the SQLite database at dbPath.
// The database is opened with WAL mode for concurrent read/write access.
func New(dbPath string) (*Store, error) {
	db, err := sql.Open("sqlite", dbPath)
	if err != nil {
		return nil, fmt.Errorf("open sqlite: %w", err)
	}

	// Enable WAL mode for append-heavy workloads with concurrent readers.
	if _, err := db.Exec("PRAGMA journal_mode=WAL"); err != nil {
		db.Close()
		return nil, fmt.Errorf("set WAL mode: %w", err)
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
		db.Close()
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
		Role:      e.Role,
		Content:   e.Content,
		Streaming: e.Streaming,
		Tool:      e.Tool,
		Target:    e.Target,
		Summary:   e.Summary,
		Command:   e.Command,
		Options:   e.Options,
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
		limit = 100
	}

	rows, err := s.db.QueryContext(ctx,
		"SELECT id, type, session_id, timestamp, payload FROM events WHERE session_id = ? AND id > ? ORDER BY id ASC LIMIT ?",
		sessionID, afterID, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("query events: %w", err)
	}
	defer rows.Close()

	return scanEvents(rows)
}

// QueryAll retrieves events across all sessions, for initial load.
func (s *Store) QueryAll(ctx context.Context, afterID int64, limit int) ([]interfaces.Event, error) {
	if limit <= 0 {
		limit = 100
	}

	rows, err := s.db.QueryContext(ctx,
		"SELECT id, type, session_id, timestamp, payload FROM events WHERE id > ? ORDER BY id ASC LIMIT ?",
		afterID, limit,
	)
	if err != nil {
		return nil, fmt.Errorf("query all events: %w", err)
	}
	defer rows.Close()

	return scanEvents(rows)
}

// eventPayload holds the variable fields of an event, stored as JSON in the payload column.
type eventPayload struct {
	Role      string   `json:"role,omitempty"`
	Content   string   `json:"content,omitempty"`
	Streaming bool     `json:"streaming,omitempty"`
	Tool      string   `json:"tool,omitempty"`
	Target    string   `json:"target,omitempty"`
	Summary   string   `json:"summary,omitempty"`
	Command   string   `json:"command,omitempty"`
	Options   []string `json:"options,omitempty"`
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

		// Parse timestamp — try common SQLite formats.
		ts, err := time.Parse("2006-01-02 15:04:05", timestampStr)
		if err != nil {
			ts, err = time.Parse(time.RFC3339, timestampStr)
			if err != nil {
				ts = time.Now().UTC()
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

		events = append(events, e)
	}

	return events, rows.Err()
}
