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
	"github.com/adama/local-agent/internal/pairing"
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
	client := acp.NewClient(acp.ClientConfig{})

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
	client := acp.NewClient(acp.ClientConfig{})
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

// TestIsMutatingMethod verifies the CSRF-relevant method classification used
// by the loopback auth-bypass CSRF check. Only state-changing methods are
// mutating; read-only methods are exempt.
func TestIsMutatingMethod(t *testing.T) {
	mutating := []string{
		http.MethodPost, http.MethodPut, http.MethodPatch, http.MethodDelete,
	}
	for _, m := range mutating {
		if !isMutatingMethod(m) {
			t.Errorf("isMutatingMethod(%q) = false, want true", m)
		}
	}
	readOnly := []string{http.MethodGet, http.MethodHead, http.MethodOptions}
	for _, m := range readOnly {
		if isMutatingMethod(m) {
			t.Errorf("isMutatingMethod(%q) = true, want false", m)
		}
	}
}

// TestLoopbackOriginAllowed verifies the Origin validation that gates mutating
// requests on the loopback auth bypass. A non-browser client (no Origin) is
// allowed; a loopback Origin (localhost / 127.0.0.1 / ::1 on any port) is
// allowed; a cross-origin Origin from a malicious website is rejected.
func TestLoopbackOriginAllowed(t *testing.T) {
	cases := []struct {
		name   string
		origin string
		want   bool
	}{
		{"no origin (non-browser CLI)", "", true},
		{"localhost any port", "http://localhost:7337", true},
		{"localhost no port", "http://localhost", true},
		{"127.0.0.1 any port", "http://127.0.0.1:7337", true},
		{"ipv6 loopback", "http://[::1]:7337", true},
		{"cross-origin attacker", "http://evil.com", false},
		{"cross-origin attacker subdomain", "http://localhost.evil.com", false},
		{"malformed origin", "://bad", false},
		{"null origin", "null", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodPost, "/api/sessions", nil)
			if tc.origin != "" {
				req.Header.Set("Origin", tc.origin)
			}
			if got := loopbackOriginAllowed(req); got != tc.want {
				t.Errorf("loopbackOriginAllowed() = %v, want %v", got, tc.want)
			}
		})
	}
}

// newLoopbackRequest builds an httptest request that the server treats as
// loopback (RemoteAddr = 127.0.0.1) so requireAuth's loopback branch is
// exercised. httptest.NewRequest defaults RemoteAddr to a non-loopback
// address, so it must be overridden explicitly.
func newLoopbackRequest(method, target string, body interface{}) *http.Request {
	var r *http.Request
	if body != nil {
		r = httptest.NewRequest(method, target, strings.NewReader(body.(string)))
	} else {
		r = httptest.NewRequest(method, target, nil)
	}
	r.RemoteAddr = "127.0.0.1:1234"
	return r
}

// TestRequireAuthLoopbackCSRF verifies the CSRF defense on the loopback auth
// bypass: a cross-origin mutating request from a malicious website (carrying
// the attacker's Origin) is rejected with 403, while the host browser
// (loopback Origin) and non-browser CLI clients (no Origin) are allowed.
// Read-only GET requests are exempt regardless of Origin.
func TestRequireAuthLoopbackCSRF(t *testing.T) {
	// A real pairing manager is required so requireAuth reaches its loopback
	// branch (PairingMgr != nil). The credential store lives in a temp dir.
	pm := pairing.NewManager(t.TempDir())

	called := false
	handler := func(w http.ResponseWriter, _ *http.Request) {
		called = true
		writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	}
	srv := &Server{deps: &Deps{PairingMgr: pm}}
	wrapped := srv.requireAuth(handler)

	// Cross-origin POST from a malicious website -> 403, handler not called.
	called = false
	req := newLoopbackRequest(http.MethodPost, "/api/sessions", `{}`)
	req.Header.Set("Origin", "http://evil.com")
	rec := httptest.NewRecorder()
	wrapped(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Errorf("cross-origin POST: expected 403, got %d", rec.Code)
	}
	if called {
		t.Error("cross-origin POST: handler should not be called")
	}

	// Host browser POST (loopback Origin) -> allowed, handler called.
	called = false
	req = newLoopbackRequest(http.MethodPost, "/api/sessions", `{}`)
	req.Header.Set("Origin", "http://localhost:7337")
	rec = httptest.NewRecorder()
	wrapped(rec, req)
	if rec.Code != http.StatusOK {
		t.Errorf("loopback-origin POST: expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}
	if !called {
		t.Error("loopback-origin POST: handler should be called")
	}

	// Non-browser CLI POST (no Origin) -> allowed, handler called.
	called = false
	req = newLoopbackRequest(http.MethodPost, "/api/sessions", `{}`)
	rec = httptest.NewRecorder()
	wrapped(rec, req)
	if rec.Code != http.StatusOK {
		t.Errorf("no-origin POST (CLI): expected 200, got %d", rec.Code)
	}
	if !called {
		t.Error("no-origin POST (CLI): handler should be called")
	}

	// Cross-origin GET -> exempt (read-only), handler called.
	called = false
	req = newLoopbackRequest(http.MethodGet, "/api/sessions", nil)
	req.Header.Set("Origin", "http://evil.com")
	rec = httptest.NewRecorder()
	wrapped(rec, req)
	if rec.Code != http.StatusOK {
		t.Errorf("cross-origin GET: expected 200 (read-only exempt), got %d", rec.Code)
	}
	if !called {
		t.Error("cross-origin GET: handler should be called (read-only exempt)")
	}

	// Cross-origin DELETE -> 403 (mutating).
	called = false
	req = newLoopbackRequest(http.MethodDelete, "/api/sessions/sess-1", nil)
	req.Header.Set("Origin", "http://evil.com")
	rec = httptest.NewRecorder()
	wrapped(rec, req)
	if rec.Code != http.StatusForbidden {
		t.Errorf("cross-origin DELETE: expected 403, got %d", rec.Code)
	}
	if called {
		t.Error("cross-origin DELETE: handler should not be called")
	}
}

// TestRequireAuthLoopbackBypassStillWorksForGET verifies that the CSRF check
// did not regress the existing loopback auth bypass for read-only requests:
// a loopback GET without any credential or Origin still reaches the handler.
func TestRequireAuthLoopbackBypassStillWorksForGET(t *testing.T) {
	pm := pairing.NewManager(t.TempDir())
	called := false
	handler := func(w http.ResponseWriter, _ *http.Request) {
		called = true
		writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
	}
	srv := &Server{deps: &Deps{PairingMgr: pm}}
	wrapped := srv.requireAuth(handler)

	req := newLoopbackRequest(http.MethodGet, "/api/devices", nil)
	rec := httptest.NewRecorder()
	wrapped(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d", rec.Code)
	}
	if !called {
		t.Error("loopback GET: handler should be called")
	}
}
