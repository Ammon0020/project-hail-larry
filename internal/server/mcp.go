package server

import (
	"encoding/json"
	"errors"
	"io"
	"net/http"
	"os"
	"strconv"

	"github.com/adama/local-agent/internal/fsutil"
	"github.com/adama/local-agent/internal/mcp"
)

// ----------------------------------------------------------------------------
// MCP Config Handlers (docs/research/mcp-config-design.md)
// ----------------------------------------------------------------------------

// emptyMcpConfigJSON is the response body returned by GET /api/mcp when no
// mcp.json exists yet. It is a valid, minimal envelope so the frontend editor
// has something to display and edit on a fresh install.
const emptyMcpConfigJSON = "{\n  \"version\": 1,\n  \"mcpServers\": {}\n}\n"

// handleGetMcp returns the raw JSON text of mcp.json. The body is returned
// unparsed (as text/plain-ish application/json) so the frontend CodeMirror
// editor preserves the user's exact formatting, key ordering, and whitespace
// on round-trips — re-parsing and re-marshalling would normalize all of that
// away. A missing file returns the empty envelope so the editor starts from a
// valid template instead of an error.
//
// Returns 503 when the server was started without an MCP config path wired in
// (degraded/test setup), and 500 on any other read error.
func (s *Server) handleGetMcp(w http.ResponseWriter, _ *http.Request) {
	if !s.requireMcpConfig(w) {
		return
	}
	path := s.mcpConfigPath()

	data, err := os.ReadFile(path) //nolint:gosec // path is constructed by the daemon from a trusted base dir.
	if err != nil {
		if os.IsNotExist(err) {
			w.Header().Set("Content-Type", "application/json")
			w.WriteHeader(http.StatusOK)
			_, _ = w.Write([]byte(emptyMcpConfigJSON))
			return
		}
		writeError(w, http.StatusInternalServerError, "read mcp config: "+err.Error())
		return
	}

	// Return the raw bytes verbatim. Content-Type is application/json so
	// browsers/fetch parse it, but the body is exactly what's on disk.
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(data)
}

// handlePutMcp replaces the entire mcp.json with the request body. The body is
// first parsed into mcp.File to validate it (catching malformed JSON and
// envelope-shape errors before the user's config is overwritten), then the
// raw request bytes are written to disk so the user's formatting is preserved
// exactly. Returns 400 on a parse/validation error with a clear message.
//
// The body size is capped at defaultMaxBodyBytes (10 MB), which is far more
// than any realistic MCP config — the limit exists to prevent a runaway
// request from exhausting memory.
func (s *Server) handlePutMcp(w http.ResponseWriter, r *http.Request) {
	if !s.requireMcpConfig(w) {
		return
	}
	path := s.mcpConfigPath()

	body := http.MaxBytesReader(w, r.Body, defaultMaxBodyBytes)
	raw, err := io.ReadAll(body)
	if err != nil {
		writeError(w, http.StatusBadRequest, "read request body: "+err.Error())
		return
	}

	// Validate by parsing into mcp.File. We do NOT write the parsed struct
	// back out — we write the raw bytes — so the user's formatting survives.
	// This catches malformed JSON and missing/extra envelope fields.
	var f mcp.File
	if err := json.Unmarshal(raw, &f); err != nil {
		writeError(w, http.StatusBadRequest, "invalid mcp config JSON: "+err.Error())
		return
	}
	if f.Version != 0 && f.Version != mcp.CurrentVersion {
		// A future migration layer would go here. For now, reject unknown
		// versions loudly rather than silently downgrading.
		writeError(w, http.StatusBadRequest, "unsupported mcp config version: "+strconv.Itoa(f.Version))
		return
	}
	if f.McpServers == nil {
		// Treat a missing mcpServers map as valid (equivalent to empty) so a
		// user can save `{"version": 1}` and add servers later.
		f.McpServers = map[string]mcp.ServerConfig{}
	}

	// Write the raw request bytes verbatim (atomically: temp + rename) so the
	// user's exact formatting, key ordering, and whitespace are preserved on
	// round-trips. mcp.Save re-marshals, which would normalize all of that
	// away; the GET endpoint returns the same raw bytes back to the editor.
	if err := fsutil.WriteFileAtomic(path, raw, 0o600); err != nil {
		writeError(w, http.StatusInternalServerError, "save mcp config: "+err.Error())
		return
	}

	// Echo the raw bytes back so the response matches the on-disk state exactly.
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write(raw)
}

// handlePatchMcpServer toggles the `enabled` flag of a single MCP server by
// name. It loads the current config, updates the named server's Enabled
// pointer, and saves. Returns 404 if the named server does not exist, 400 on a
// malformed body, and 503 when MCP config is not wired in.
//
// This is the backend for the per-server enable/disable toggle in the UI; the
// full JSON editor uses PUT /api/mcp instead.
func (s *Server) handlePatchMcpServer(w http.ResponseWriter, r *http.Request) {
	if !s.requireMcpConfig(w) {
		return
	}
	path := s.mcpConfigPath()

	name := r.PathValue("name")
	if name == "" {
		writeError(w, http.StatusBadRequest, "missing server name")
		return
	}

	var req struct {
		Enabled *bool `json:"enabled"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		// An empty body is not valid for PATCH /servers/{name} — the caller
		// must specify enabled. Distinguish EOF from a malformed body for a
		// clearer error message.
		if errors.Is(err, io.EOF) {
			writeError(w, http.StatusBadRequest, "missing 'enabled' field in request body")
			return
		}
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if req.Enabled == nil {
		writeError(w, http.StatusBadRequest, "missing 'enabled' field in request body")
		return
	}

	f, err := mcp.Load(path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "load mcp config: "+err.Error())
		return
	}
	cfg, ok := f.McpServers[name]
	if !ok {
		writeError(w, http.StatusNotFound, "mcp server not found: "+name)
		return
	}
	cfg.Enabled = req.Enabled
	f.McpServers[name] = cfg

	if err := mcp.Save(path, f); err != nil {
		writeError(w, http.StatusInternalServerError, "save mcp config: "+err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]any{
		"name":    name,
		"enabled": *req.Enabled,
	})
}

// mcpConfigPath returns the MCP config file path from the server deps, or "" if
// none is configured. Centralizing the lookup keeps the handlers terse and
// makes the "not configured" guard a single check.
func (s *Server) mcpConfigPath() string {
	if s.deps == nil {
		return ""
	}
	return s.deps.McpConfigPath
}

// handleGetMcpStatus performs on-demand health checks for all configured MCP
// servers and returns a JSON array of their statuses. stdio servers are checked
// via exec.LookPath (binary exists?); http/sse servers via TCP dial. Disabled
// servers are reported as "disabled" without performing any check.
//
// This is called by the frontend when the MCP popout opens — there is no
// background health-check ticker, keeping the daemon lightweight.
func (s *Server) handleGetMcpStatus(w http.ResponseWriter, _ *http.Request) {
	if !s.requireMcpConfig(w) {
		return
	}
	path := s.mcpConfigPath()

	f, err := mcp.Load(path)
	if err != nil {
		writeError(w, http.StatusInternalServerError, "load mcp config: "+err.Error())
		return
	}

	statuses := mcp.CheckHealth(f, 0)

	// Return an empty array instead of null when no servers are configured.
	if statuses == nil {
		statuses = []mcp.ServerStatus{}
	}

	writeJSON(w, http.StatusOK, statuses)
}
