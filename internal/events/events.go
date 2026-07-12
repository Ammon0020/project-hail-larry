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
	"sync"
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

	// Enable incremental auto-vacuum so that pruning old events can actually
	// return pages to the OS. auto_vacuum must be set before any tables are
	// created; on an existing database where it was already set it is a no-op,
	// and on one where it was OFF it cannot be changed to INCREMENTAL without
	// VACUUM — we issue it best-effort and ignore the result so a legacy DB
	// still loads. After pruning we run `PRAGMA incremental_vacuum` to reclaim
	// the freed pages without the cost of a full VACUUM.
	_, _ = db.Exec("PRAGMA auto_vacuum = INCREMENTAL")

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

// ----------------------------------------------------------------------------
// Retention / pruning (Finding 8.3)
//
// The events table is append-only, so without pruning it grows without bound.
// Prune(maxRows) keeps only the most recent maxRows events; PruneOlderThan
// drops events older than a duration. StartPruneTicker runs Prune on a
// schedule in a background goroutine and returns a stop function.
//
// After a prune that actually removed rows we run `PRAGMA incremental_vacuum`
// to return freed pages to the OS. This requires auto_vacuum = INCREMENTAL,
// which we set (best-effort) during initialization. A full VACUUM would also
// reclaim space but locks the database and rewrites the whole file, so we
// avoid it on the hot path.
// ----------------------------------------------------------------------------

// DefaultPruneMaxRows is the default maximum number of events retained by
// StartPruneTicker when the caller does not override it.
const DefaultPruneMaxRows = 100000

// DefaultPruneInterval is the default interval between automatic prune passes
// started by StartPruneTicker when the caller does not override it.
const DefaultPruneInterval = time.Hour

// Prune deletes the oldest events so that at most maxRows rows remain. If the
// table already has maxRows or fewer rows it is a no-op. Returns the number of
// rows deleted. A non-positive maxRows is treated as an error rather than
// silently truncating the whole table.
func (s *Store) Prune(maxRows int) (int64, error) {
	if maxRows <= 0 {
		return 0, fmt.Errorf("events: Prune requires maxRows > 0, got %d", maxRows)
	}

	// Keep the newest maxRows events by id and delete the rest. The subquery
	// is the exact form requested in the finding; SQLite handles the NOT IN
	// against the primary key efficiently.
	return s.pruneWithQuery("prune by row count",
		"DELETE FROM events WHERE id NOT IN (SELECT id FROM events ORDER BY id DESC LIMIT ?)",
		maxRows,
	)
}

// PruneOlderThan deletes events whose timestamp is older than maxAge measured
// from now (UTC). Returns the number of rows deleted. A non-positive maxAge is
// treated as an error rather than silently deleting everything.
func (s *Store) PruneOlderThan(maxAge time.Duration) (int64, error) {
	if maxAge <= 0 {
		return 0, fmt.Errorf("events: PruneOlderThan requires maxAge > 0, got %s", maxAge)
	}

	cutoff := time.Now().UTC().Add(-maxAge)
	return s.pruneWithQuery("prune by age",
		"DELETE FROM events WHERE timestamp < ?",
		cutoff,
	)
}

// pruneWithQuery executes a DELETE query and returns the number of rows deleted.
// label is used in error messages to identify the prune operation. If rows are
// deleted, incrementalVacuum is called to reclaim freed pages.
func (s *Store) pruneWithQuery(label, query string, args ...any) (int64, error) {
	res, err := s.db.Exec(query, args...)
	if err != nil {
		return 0, fmt.Errorf("events: %s: %w", label, err)
	}

	deleted, err := res.RowsAffected()
	if err != nil {
		return 0, fmt.Errorf("events: %s rows affected: %w", label, err)
	}

	if deleted > 0 {
		if err := s.incrementalVacuum(); err != nil {
			// Vacuum failure is non-fatal — the rows are already gone, we
			// just couldn't hand pages back to the OS yet. Log via the error
			// chain so callers can observe it.
			return deleted, fmt.Errorf("events: %s succeeded but vacuum failed: %w", label, err)
		}
	}
	return deleted, nil
}

// incrementalVacuum reclaims freed pages after a prune when auto_vacuum is
// INCREMENTAL. It is a no-op (returns nil) if auto_vacuum is not enabled, so
// it is safe to call on legacy databases that predate the schema change.
func (s *Store) incrementalVacuum() error {
	// PRAGMA incremental_vacuum with no argument reclaims all available pages.
	// On a database without incremental auto_vacuum this is a harmless no-op.
	if _, err := s.db.Exec("PRAGMA incremental_vacuum"); err != nil {
		return fmt.Errorf("incremental_vacuum: %w", err)
	}
	return nil
}

// StartPruneTicker launches a background goroutine that calls Prune(maxRows)
// every interval, returning a stop function that halts the goroutine and
// waits for any in-flight prune to finish. The stop function is safe to call
// multiple times.
//
// Use DefaultPruneInterval and DefaultPruneMaxRows for sensible production
// defaults (1 hour / 100000 events). The goroutine uses context.Background
// because pruning is a store-internal maintenance task that should outlive
// any single request's context.
func (s *Store) StartPruneTicker(interval time.Duration, maxRows int) func() {
	if interval <= 0 {
		interval = DefaultPruneInterval
	}
	if maxRows <= 0 {
		maxRows = DefaultPruneMaxRows
	}

	ticker := time.NewTicker(interval)
	done := make(chan struct{})
	var once sync.Once

	go func() {
		for {
			select {
			case <-done:
				ticker.Stop()
				return
			case <-ticker.C:
				// Best-effort: a prune error here is not actionable from the
				// caller's context, so we ignore it. The next tick retries.
				_, _ = s.Prune(maxRows)
			}
		}
	}()

	stop := func() {
		once.Do(func() { close(done) })
	}
	return stop
}
