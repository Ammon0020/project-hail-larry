package server

import (
	"bytes"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/adama/local-agent/internal/mcp"
)

// newMcpServer builds a Server wired with an MCP config path inside a temp dir.
// It returns the server and the on-disk mcp.json path.
func newMcpServer(t *testing.T) (*Server, string) {
	t.Helper()
	dir := t.TempDir()
	path := filepath.Join(dir, "mcp.json")
	srv := New(&Deps{McpConfigPath: path})
	return srv, path
}

// TestGetMcpMissingFile verifies GET /api/mcp returns the empty envelope when
// no mcp.json exists yet, so the frontend editor starts from a valid template.
func TestGetMcpMissingFile(t *testing.T) {
	srv, _ := newMcpServer(t)
	req := httptest.NewRequest(http.MethodGet, "/api/mcp", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200", rec.Code)
	}
	var f mcp.File
	if err := json.Unmarshal(rec.Body.Bytes(), &f); err != nil {
		t.Fatalf("body is not valid JSON: %v; body=%s", err, rec.Body.String())
	}
	if f.Version != mcp.CurrentVersion {
		t.Errorf("Version = %d, want %d", f.Version, mcp.CurrentVersion)
	}
	if f.McpServers == nil {
		t.Error("McpServers is nil, want empty map")
	}
}

// TestPutMcpValidatesAndSaves verifies PUT /api/mcp validates the body by
// parsing into mcp.File, then writes the raw bytes to disk so the user's
// formatting is preserved.
func TestPutMcpValidatesAndSaves(t *testing.T) {
	srv, path := newMcpServer(t)

	// Body with non-canonical formatting (extra whitespace, key order). The
	// raw bytes must be preserved on disk.
	body := []byte("{\n  \"version\": 1,\n  \"mcpServers\": {\n    \"github\": {\n      \"command\": \"npx\",\n      \"args\": [\"-y\", \"pkg\"]\n    }\n  }\n}\n")
	req := httptest.NewRequest(http.MethodPut, "/api/mcp", bytes.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
	onDisk, err := os.ReadFile(path)
	if err != nil {
		t.Fatalf("ReadFile: %v", err)
	}
	if !bytes.Equal(onDisk, body) {
		t.Errorf("on-disk bytes do not match request body (formatting not preserved):\n got=%q\nwant=%q", string(onDisk), string(body))
	}
}

// TestPutMcpRejectsInvalidJSON verifies PUT /api/mcp returns 400 with a clear
// message when the body is not valid JSON.
func TestPutMcpRejectsInvalidJSON(t *testing.T) {
	srv, _ := newMcpServer(t)
	req := httptest.NewRequest(http.MethodPut, "/api/mcp", bytes.NewReader([]byte("{not json")))
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusBadRequest {
		t.Fatalf("status = %d, want 400", rec.Code)
	}
	var resp map[string]string
	if err := json.Unmarshal(rec.Body.Bytes(), &resp); err != nil {
		t.Fatalf("body not JSON: %v", err)
	}
	if _, ok := resp["error"]; !ok {
		t.Error("response missing 'error' field")
	}
}

// TestPatchMcpServerToggle verifies PATCH /api/mcp/servers/{name} updates the
// enabled flag of a single server and persists the change.
func TestPatchMcpServerToggle(t *testing.T) {
	srv, path := newMcpServer(t)

	// Seed a config with one server (enabled=nil => default on).
	seed := &mcp.File{
		Version: mcp.CurrentVersion,
		McpServers: map[string]mcp.ServerConfig{
			"github": {Command: "npx", Args: []string{"-y", "pkg"}},
		},
	}
	if err := mcp.Save(path, seed); err != nil {
		t.Fatalf("seed Save: %v", err)
	}

	// Disable it.
	body := []byte(`{"enabled": false}`)
	req := httptest.NewRequest(http.MethodPatch, "/api/mcp/servers/github", bytes.NewReader(body))
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("disable: status = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}

	loaded, err := mcp.Load(path)
	if err != nil {
		t.Fatalf("Load: %v", err)
	}
	cfg, ok := loaded.McpServers["github"]
	if !ok {
		t.Fatal("server 'github' missing after patch")
	}
	if cfg.Enabled == nil || *cfg.Enabled != false {
		t.Errorf("after disable: Enabled = %v, want &false", cfg.Enabled)
	}

	// Re-enable it.
	body = []byte(`{"enabled": true}`)
	req = httptest.NewRequest(http.MethodPatch, "/api/mcp/servers/github", bytes.NewReader(body))
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("enable: status = %d, want 200; body=%s", rec.Code, rec.Body.String())
	}
	loaded, _ = mcp.Load(path)
	cfg = loaded.McpServers["github"]
	if cfg.Enabled == nil || *cfg.Enabled != true {
		t.Errorf("after enable: Enabled = %v, want &true", cfg.Enabled)
	}
}

// TestPatchMcpServerNotFound verifies PATCH on a non-existent server returns 404.
func TestPatchMcpServerNotFound(t *testing.T) {
	srv, path := newMcpServer(t)
	if err := mcp.Save(path, mcp.NewFile()); err != nil {
		t.Fatalf("Save: %v", err)
	}
	req := httptest.NewRequest(http.MethodPatch, "/api/mcp/servers/nope", bytes.NewReader([]byte(`{"enabled": true}`)))
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusNotFound {
		t.Errorf("status = %d, want 404; body=%s", rec.Code, rec.Body.String())
	}
}

// TestMcpEndpointsNotConfigured verifies the MCP endpoints return 503 when the
// server was started without an MCP config path (degraded/test setup).
func TestMcpEndpointsNotConfigured(t *testing.T) {
	srv := New(&Deps{}) // no McpConfigPath

	req := httptest.NewRequest(http.MethodGet, "/api/mcp", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusServiceUnavailable {
		t.Errorf("GET /api/mcp: status = %d, want 503", rec.Code)
	}

	req = httptest.NewRequest(http.MethodPut, "/api/mcp", bytes.NewReader([]byte(`{}`)))
	rec = httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusServiceUnavailable {
		t.Errorf("PUT /api/mcp: status = %d, want 503", rec.Code)
	}
}

// TestGetMcpReturnsRawBytes verifies GET /api/mcp returns the exact on-disk
// bytes (not a re-marshaled version) so the frontend editor preserves
// formatting on round-trips.
func TestGetMcpReturnsRawBytes(t *testing.T) {
	srv, path := newMcpServer(t)
	raw := []byte("{\n  \"version\": 1,\n  \"mcpServers\": {\n    \"a\": {\n      \"command\": \"c\"\n    }\n  }\n}\n")
	if err := os.WriteFile(path, raw, 0o600); err != nil {
		t.Fatalf("WriteFile: %v", err)
	}
	req := httptest.NewRequest(http.MethodGet, "/api/mcp", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)
	if rec.Code != http.StatusOK {
		t.Fatalf("status = %d, want 200", rec.Code)
	}
	body, _ := io.ReadAll(rec.Result().Body)
	if !bytes.Equal(body, raw) {
		t.Errorf("body does not match raw on-disk bytes:\n got=%q\nwant=%q", string(body), string(raw))
	}
}
