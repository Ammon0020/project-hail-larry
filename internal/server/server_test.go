package server

import (
	"net/http"
	"net/http/httptest"
	"testing"
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
