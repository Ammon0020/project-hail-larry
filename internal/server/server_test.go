package server

import (
	"context"
	"net/http"
	"net/http/httptest"
	"path/filepath"
	"testing"

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
