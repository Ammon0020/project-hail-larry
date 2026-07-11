package server

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/events"
	"github.com/adama/local-agent/internal/interfaces"
)

// TestHealthCheck verifies the /health endpoint returns 200 OK with JSON.
func TestHealthCheck(t *testing.T) {
	srv := New(nil)
	req := httptest.NewRequest(http.MethodGet, "/health", nil)
	rec := httptest.NewRecorder()

	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Errorf("expected status 200, got %d", rec.Code)
	}

	expected := `{"status":"ok"}` + "\n"
	if rec.Body.String() != expected {
		t.Errorf("expected body %q, got %q", expected, rec.Body.String())
	}
}

// TestFrontendServed verifies the SPA fallback serves index.html for unknown routes.
func TestFrontendServed(t *testing.T) {
	srv := New(nil)
	req := httptest.NewRequest(http.MethodGet, "/some-spa-route", nil)
	rec := httptest.NewRecorder()

	srv.Handler().ServeHTTP(rec, req)

	// Should serve the placeholder index.html (or the real build if present).
	if rec.Code != http.StatusOK {
		t.Errorf("expected status 200 for SPA fallback, got %d", rec.Code)
	}
}

func TestOnEventPersistsACPEvent(t *testing.T) {
	store, err := events.New(filepath.Join(t.TempDir(), "events.db"))
	if err != nil {
		t.Fatalf("new event store: %v", err)
	}
	defer func() { _ = store.Close() }()

	srv := New(&Deps{EventStore: store})
	srv.OnEvent(interfaces.Event{
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "session-1",
		Role:      "user",
		Content:   "hello",
	})

	got, err := store.Query(context.Background(), "session-1", 0, 10)
	if err != nil {
		t.Fatalf("query events: %v", err)
	}
	if len(got) != 1 {
		t.Fatalf("expected one event, got %d", len(got))
	}
	if got[0].Type != interfaces.EventPromptSubmitted || got[0].Content != "hello" {
		t.Fatalf("unexpected event: %+v", got[0])
	}
}

// TestGetSessionEventsDefaultLimit verifies that GET /api/events/{sessionId}
// returns up to 1000 events by default (raised from 100 so long streaming
// responses are not truncated), and that ?limit still constrains the result.
func TestGetSessionEventsDefaultLimit(t *testing.T) {
	store, err := events.New(filepath.Join(t.TempDir(), "events.db"))
	if err != nil {
		t.Fatalf("new event store: %v", err)
	}
	defer func() { _ = store.Close() }()

	ctx := context.Background()
	const total = 250
	for i := 0; i < total; i++ {
		if _, err := store.Append(ctx, interfaces.Event{
			Type:      interfaces.EventStreamUpdate,
			SessionID: "sess-bulk",
			Content:   "chunk",
		}); err != nil {
			t.Fatalf("append %d: %v", i, err)
		}
	}

	srv := New(&Deps{EventStore: store})

	// Default limit (no ?limit param) should return all 250 events.
	req := httptest.NewRequest(http.MethodGet, "/api/events/sess-bulk", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	var got []interfaces.Event
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(got) != total {
		t.Errorf("default limit: expected %d events, got %d (default limit should be 1000)", total, len(got))
	}

	// ?limit=10 should still constrain the result to 10.
	req = httptest.NewRequest(http.MethodGet, "/api/events/sess-bulk?limit=10", nil)
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	got = got[:0]
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(got) != 10 {
		t.Errorf("?limit=10: expected 10 events, got %d", len(got))
	}
}

// TestGetSession verifies GET /api/sessions/{id} returns the correct session
// metadata and that a nonexistent ID yields 404. Sessions are loaded from a
// temp JSON file via LoadConversations so no real agent process is spawned.
func TestGetSession(t *testing.T) {
	// Build a minimal acp.Client and inject a session through the persistence
	// layer (LoadConversations) — this avoids spawning a real agent transport,
	// which CreateSession would require.
	client := acp.NewClient(nil, nil)

	now := time.Now().UTC().Truncate(time.Second)
	seed := []acp.Session{{
		ID:        "sess-test-123",
		Name:      "My test chat",
		AgentID:   "agent-1",
		ModelID:   "model-a",
		Workspace: "ws-1",
		Status:    "idle",
		CreatedAt: now,
		UpdatedAt: now,
	}}
	data, err := json.MarshalIndent(seed, "", "  ")
	if err != nil {
		t.Fatalf("marshal seed sessions: %v", err)
	}
	storePath := filepath.Join(t.TempDir(), "conversations.json")
	if err := os.WriteFile(storePath, data, 0600); err != nil {
		t.Fatalf("write store file: %v", err)
	}
	client.SetStorePath(storePath)
	if err := client.LoadConversations(); err != nil {
		t.Fatalf("load conversations: %v", err)
	}

	srv := New(&Deps{ACPClient: client})

	// Happy path: existing session returns 200 with correct fields.
	req := httptest.NewRequest(http.MethodGet, "/api/sessions/sess-test-123", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}

	var info interfaces.SessionInfo
	if err := json.Unmarshal(rec.Body.Bytes(), &info); err != nil {
		t.Fatalf("unmarshal response: %v", err)
	}
	if info.ID != "sess-test-123" {
		t.Errorf("expected id %q, got %q", "sess-test-123", info.ID)
	}
	if info.Name != "My test chat" {
		t.Errorf("expected name %q, got %q", "My test chat", info.Name)
	}
	if info.AgentID != "agent-1" {
		t.Errorf("expected agentId %q, got %q", "agent-1", info.AgentID)
	}
	if info.Status != "idle" {
		t.Errorf("expected status %q, got %q", "idle", info.Status)
	}

	// Not-found path: nonexistent ID returns 404.
	req = httptest.NewRequest(http.MethodGet, "/api/sessions/nonexistent", nil)
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for nonexistent session, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestExportSession verifies GET /api/sessions/{id}/export renders the
// session's event history as a markdown transcript and returns it as a
// text/markdown attachment. The filename is derived from the session name
// (sanitized), and a nonexistent session yields 404. The session is loaded
// via LoadConversations so no real agent transport is spawned (mirrors
// TestGetSession).
func TestExportSession(t *testing.T) {
	// Build a minimal acp.Client with one seeded session.
	client := acp.NewClient(nil, nil)
	now := time.Now().UTC().Truncate(time.Second)
	seed := []acp.Session{{
		ID:        "sess-export-1",
		Name:      "Export / Test: Chat?",
		AgentID:   "agent-1",
		ModelID:   "model-a",
		Workspace: "ws-1",
		Status:    "idle",
		CreatedAt: now,
		UpdatedAt: now,
	}}
	data, err := json.MarshalIndent(seed, "", "  ")
	if err != nil {
		t.Fatalf("marshal seed sessions: %v", err)
	}
	storePath := filepath.Join(t.TempDir(), "conversations.json")
	if err = os.WriteFile(storePath, data, 0600); err != nil {
		t.Fatalf("write store file: %v", err)
	}
	client.SetStorePath(storePath)
	if err = client.LoadConversations(); err != nil {
		t.Fatalf("load conversations: %v", err)
	}

	// Seed the event store with a user prompt and an assistant stream chunk so
	// the rendered transcript has content to assert on.
	eventStore, err := events.New(filepath.Join(t.TempDir(), "events.db"))
	if err != nil {
		t.Fatalf("new event store: %v", err)
	}
	defer func() { _ = eventStore.Close() }()
	ctx := context.Background()
	if _, err := eventStore.Append(ctx, interfaces.Event{
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "sess-export-1",
		Content:   "Hello, agent!",
	}); err != nil {
		t.Fatalf("append prompt: %v", err)
	}
	if _, err := eventStore.Append(ctx, interfaces.Event{
		Type:      interfaces.EventStreamUpdate,
		SessionID: "sess-export-1",
		Content:   "Hi there, user.",
	}); err != nil {
		t.Fatalf("append stream: %v", err)
	}

	srv := New(&Deps{EventStore: eventStore, ACPClient: client})

	req := httptest.NewRequest(http.MethodGet, "/api/sessions/sess-export-1/export", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}

	// Content-Type must be markdown.
	if ct := rec.Header().Get("Content-Type"); !strings.HasPrefix(ct, "text/markdown") {
		t.Errorf("expected text/markdown content-type, got %q", ct)
	}

	// Content-Disposition must be an attachment with a sanitized filename
	// derived from the session name. The slashes, colons, and question mark in
	// "Export / Test: Chat?" are replaced with underscores, and trailing
	// underscores are trimmed by sanitizeFilename.
	cd := rec.Header().Get("Content-Disposition")
	const wantCD = `attachment; filename="Export___Test__Chat.md"`
	if cd != wantCD {
		t.Errorf("expected content-disposition %q, got %q", wantCD, cd)
	}

	// Body must contain the rendered transcript: a User line and an Assistant
	// line. ExportConversation formats these as "**User:** ..." and
	// "**Assistant:** ...".
	body := rec.Body.String()
	if !strings.Contains(body, "**User:** Hello, agent!") {
		t.Errorf("expected body to contain user prompt, got %q", body)
	}
	if !strings.Contains(body, "**Assistant:** Hi there, user.") {
		t.Errorf("expected body to contain assistant response, got %q", body)
	}

	// Not-found path: nonexistent session yields 404.
	req = httptest.NewRequest(http.MethodGet, "/api/sessions/nonexistent/export", nil)
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for nonexistent session, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestHandleSessionContext_Selection verifies POST /api/sessions/{id}/context
// accepts and stores an editor selection on the OpenFilesTracker, and that
// omitting the selection field leaves any prior selection unchanged.
func TestHandleSessionContext_Selection(t *testing.T) {
	tracker := acp.NewOpenFilesTracker()
	srv := New(&Deps{OpenFilesTracker: tracker})

	body := `{"openFiles":["a.go"],"selection":{"path":"a.go","startLine":2,"endLine":4,"text":"selected"}}`
	req := httptest.NewRequest(http.MethodPost, "/api/sessions/sess-1/context", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}

	sel := tracker.Selection()
	if sel.Path != "a.go" {
		t.Errorf("expected selection path a.go, got %q", sel.Path)
	}
	if sel.StartLine != 2 || sel.EndLine != 4 {
		t.Errorf("expected selection lines 2-4, got %d-%d", sel.StartLine, sel.EndLine)
	}
	if sel.Text != "selected" {
		t.Errorf("expected selection text 'selected', got %q", sel.Text)
	}
	if of := tracker.OpenFiles(); len(of) != 1 || of[0] != "a.go" {
		t.Errorf("expected open files [a.go], got %v", of)
	}

	// A follow-up request without the selection field must NOT clear it
	// (omitted fields leave the tracker unchanged).
	body2 := `{"recentEdits":["b.go"]}`
	req2 := httptest.NewRequest(http.MethodPost, "/api/sessions/sess-1/context", strings.NewReader(body2))
	req2.Header.Set("Content-Type", "application/json")
	rec2 := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec2, req2)
	if rec2.Code != http.StatusOK {
		t.Fatalf("expected 200 on second post, got %d (body: %s)", rec2.Code, rec2.Body.String())
	}
	sel2 := tracker.Selection()
	if sel2.Text != "selected" {
		t.Errorf("expected selection to be unchanged when omitted, got %q", sel2.Text)
	}
	if re := tracker.RecentEdits(); len(re) != 1 || re[0] != "b.go" {
		t.Errorf("expected recent edits [b.go], got %v", re)
	}

	// An explicit empty-text selection clears the stored selection.
	body3 := `{"selection":{"path":"","startLine":0,"endLine":0,"text":""}}`
	req3 := httptest.NewRequest(http.MethodPost, "/api/sessions/sess-1/context", strings.NewReader(body3))
	req3.Header.Set("Content-Type", "application/json")
	rec3 := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec3, req3)
	if rec3.Code != http.StatusOK {
		t.Fatalf("expected 200 on clear post, got %d (body: %s)", rec3.Code, rec3.Body.String())
	}
	sel3 := tracker.Selection()
	if sel3.Text != "" {
		t.Errorf("expected cleared selection, got %+v", sel3)
	}
}
