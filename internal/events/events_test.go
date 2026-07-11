package events

import (
	"context"
	"path/filepath"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
)

// newTestStore creates a temporary SQLite event store for testing.
func newTestStore(t *testing.T) *Store {
	t.Helper()
	dbPath := filepath.Join(t.TempDir(), "test_events.db")
	store, err := New(dbPath)
	if err != nil {
		t.Fatalf("create store: %v", err)
	}
	t.Cleanup(func() { store.Close() })
	return store
}

// TestAppendAndQuery verifies that events can be appended and queried back.
func TestAppendAndQuery(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	// Append a user prompt event.
	e1, err := store.Append(ctx, interfaces.Event{
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "session-1",
		Role:      "user",
		Content:   "Hello, agent!",
	})
	if err != nil {
		t.Fatalf("append event: %v", err)
	}
	if e1.ID == 0 {
		t.Error("expected non-zero event ID")
	}

	// Append an agent response event.
	e2, err := store.Append(ctx, interfaces.Event{
		Type:      interfaces.EventResponseStarted,
		SessionID: "session-1",
		Role:      "agent",
		Content:   "Hello, human!",
	})
	if err != nil {
		t.Fatalf("append event: %v", err)
	}
	if e2.ID <= e1.ID {
		t.Errorf("expected e2.ID > e1.ID, got %d <= %d", e2.ID, e1.ID)
	}

	// Query all events for session-1.
	events, err := store.Query(ctx, "session-1", 0, 100)
	if err != nil {
		t.Fatalf("query events: %v", err)
	}
	if len(events) != 2 {
		t.Fatalf("expected 2 events, got %d", len(events))
	}

	// Verify first event.
	if events[0].Type != interfaces.EventPromptSubmitted {
		t.Errorf("expected first event type PromptSubmitted, got %s", events[0].Type)
	}
	if events[0].Content != "Hello, agent!" {
		t.Errorf("expected content 'Hello, agent!', got %s", events[0].Content)
	}
	if events[0].Role != "user" {
		t.Errorf("expected role 'user', got %s", events[0].Role)
	}

	// Verify second event.
	if events[1].Type != interfaces.EventResponseStarted {
		t.Errorf("expected second event type ResponseStarted, got %s", events[1].Type)
	}
	if events[1].Content != "Hello, human!" {
		t.Errorf("expected content 'Hello, human!', got %s", events[1].Content)
	}
}

// TestQueryWithCursor verifies that the afterID cursor filters correctly.
func TestQueryWithCursor(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	// Append three events.
	e1, _ := store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s1", Content: "first"})
	e2, _ := store.Append(ctx, interfaces.Event{Type: interfaces.EventResponseStarted, SessionID: "s1", Content: "second"})
	store.Append(ctx, interfaces.Event{Type: interfaces.EventStreamUpdate, SessionID: "s1", Content: "third"})

	// Query events after e1 (should get e2 and e3).
	events, err := store.Query(ctx, "s1", e1.ID, 100)
	if err != nil {
		t.Fatalf("query events: %v", err)
	}
	if len(events) != 2 {
		t.Fatalf("expected 2 events after cursor, got %d", len(events))
	}
	if events[0].ID != e2.ID {
		t.Errorf("expected first event ID %d, got %d", e2.ID, events[0].ID)
	}
}

// TestQueryDifferentSessions verifies events are isolated by session ID.
func TestQueryDifferentSessions(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s1", Content: "session 1"})
	store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s2", Content: "session 2"})
	store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s1", Content: "session 1 again"})

	// Query session 1.
	events1, err := store.Query(ctx, "s1", 0, 100)
	if err != nil {
		t.Fatalf("query s1: %v", err)
	}
	if len(events1) != 2 {
		t.Fatalf("expected 2 events for s1, got %d", len(events1))
	}

	// Query session 2.
	events2, err := store.Query(ctx, "s2", 0, 100)
	if err != nil {
		t.Fatalf("query s2: %v", err)
	}
	if len(events2) != 1 {
		t.Fatalf("expected 1 event for s2, got %d", len(events2))
	}
}

// TestQueryAll verifies cross-session retrieval.
func TestQueryAll(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s1", Content: "a"})
	store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s2", Content: "b"})
	store.Append(ctx, interfaces.Event{Type: interfaces.EventPromptSubmitted, SessionID: "s3", Content: "c"})

	events, err := store.QueryAll(ctx, 0, 100)
	if err != nil {
		t.Fatalf("query all: %v", err)
	}
	if len(events) != 3 {
		t.Fatalf("expected 3 events, got %d", len(events))
	}
}

// TestQueryEmpty verifies querying a session with no events returns empty.
func TestQueryEmpty(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	events, err := store.Query(ctx, "nonexistent", 0, 100)
	if err != nil {
		t.Fatalf("query empty: %v", err)
	}
	if len(events) != 0 {
		t.Errorf("expected 0 events, got %d", len(events))
	}
}

// TestAppendToolEvent verifies tool-specific fields are persisted and retrieved.
func TestAppendToolEvent(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	_, err := store.Append(ctx, interfaces.Event{
		Type:      interfaces.EventToolCompleted,
		SessionID: "s1",
		Tool:      "edit_file",
		Target:    "server.js",
		Summary:   "Added error handler at line 17-21",
	})
	if err != nil {
		t.Fatalf("append tool event: %v", err)
	}

	events, err := store.Query(ctx, "s1", 0, 100)
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}

	e := events[0]
	if e.Tool != "edit_file" {
		t.Errorf("expected tool 'edit_file', got %s", e.Tool)
	}
	if e.Target != "server.js" {
		t.Errorf("expected target 'server.js', got %s", e.Target)
	}
	if e.Summary != "Added error handler at line 17-21" {
		t.Errorf("expected summary, got %s", e.Summary)
	}
}

// TestAppendAttachmentsEvent verifies attachments survive the append→query
// round-trip with all fields intact (ID, Name, MimeType, Path).
func TestAppendAttachmentsEvent(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	_, err := store.Append(ctx, interfaces.Event{
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "s1",
		Role:      "user",
		Content:   "see attached",
		Attachments: []interfaces.Attachment{{
			ID:       "abc123def456",
			Name:     "test.png",
			MimeType: "image/png",
			Path:     "/some/path",
		}},
	})
	if err != nil {
		t.Fatalf("append attachments event: %v", err)
	}

	events, err := store.Query(ctx, "s1", 0, 100)
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}
	if len(events[0].Attachments) != 1 {
		t.Fatalf("expected 1 attachment, got %d", len(events[0].Attachments))
	}

	a := events[0].Attachments[0]
	if a.ID != "abc123def456" {
		t.Errorf("expected ID 'abc123def456', got %q", a.ID)
	}
	if a.Name != "test.png" {
		t.Errorf("expected Name 'test.png', got %q", a.Name)
	}
	if a.MimeType != "image/png" {
		t.Errorf("expected MimeType 'image/png', got %q", a.MimeType)
	}
	if a.Path != "/some/path" {
		t.Errorf("expected Path '/some/path', got %q", a.Path)
	}
}

// TestAppendPermissionEvent verifies permission-specific fields are persisted.
func TestAppendPermissionEvent(t *testing.T) {
	store := newTestStore(t)
	ctx := context.Background()

	_, err := store.Append(ctx, interfaces.Event{
		Type:      interfaces.EventPermissionRequested,
		SessionID: "s1",
		Tool:      "shell",
		Command:   "npm test",
		Options:   []string{"allow_once", "allow_session", "allow_always", "deny"},
	})
	if err != nil {
		t.Fatalf("append permission event: %v", err)
	}

	events, err := store.Query(ctx, "s1", 0, 100)
	if err != nil {
		t.Fatalf("query: %v", err)
	}
	if len(events) != 1 {
		t.Fatalf("expected 1 event, got %d", len(events))
	}

	e := events[0]
	if e.Command != "npm test" {
		t.Errorf("expected command 'npm test', got %s", e.Command)
	}
	if len(e.Options) != 4 {
		t.Errorf("expected 4 options, got %d", len(e.Options))
	}
}
