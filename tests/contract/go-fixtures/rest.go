// Package main: REST route fixture capture.
//
// Each supported REST route is exercised in-process via httptest.NewRequest +
// httptest.NewRecorder so the RemoteAddr can be controlled. This is what lets
// the harness capture both the loopback auth-bypass path (success cases) and
// the non-loopback unauthenticated rejection path (401 cases) from the same
// fully-wired server, without depending on a real LAN interface.
//
// Every captured response is written to golden/rest/<name>.json as:
//
//	{
//	  "method": "GET",
//	  "path":   "/health",
//	  "status": 200,
//	  "contentType": "application/json",
//	  "body": "<redacted raw body string>"
//	}
//
// The body is the raw response body (redacted). JSON bodies are kept as raw
// text so the future Rust differential runner can choose semantic (parsed)
// comparison for object/array responses and exact byte comparison for
// contractually-significant text (error messages, markdown exports).
package main

import (
	"encoding/json"
	"fmt"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
)

// restFixture is the on-disk shape of a single REST golden fixture.
type restFixture struct {
	Method      string `json:"method"`
	Path        string `json:"path"`
	Status      int    `json:"status"`
	ContentType string `json:"contentType"`
	Body        string `json:"body"`
}

// restCase describes one REST request to capture.
type restCase struct {
	// name is the golden filename (without extension).
	name string
	// method is the HTTP method.
	method string
	// path is the request path (may include query string).
	path string
	// body is the request body, or "" for no body.
	body string
	// loopback controls the request's RemoteAddr. true => "127.0.0.1:1234"
	// (auth bypass, the host-browser path); false => "10.0.0.7:1234" (a
	// remote LAN device, subject to device-credential auth).
	loopback bool
	// authHeader is an optional Authorization header value. When set on a
	// non-loopback request, the auth middleware validates it. Use a fake
	// "Bearer x:y" to exercise the 401 rejection path.
	authHeader string
	// origin is an optional Origin header. When set on a mutating loopback
	// request, the CSRF Origin check is exercised (cross-origin => 403).
	origin string
}

// captureREST runs every REST case against the harness server and writes
// golden/rest/<name>.json for each. It returns the first error encountered.
func captureREST(h *harness, goldenDir string) error {
	outDir := filepath.Join(goldenDir, "rest")
	if err := os.MkdirAll(outDir, 0o755); err != nil {
		return fmt.Errorf("mkdir %s: %w", outDir, err)
	}

	// Seed a workspace so the workspace-scoped routes have a real target. The
	// workspace ID is deterministic (hash of the absolute path), so it is
	// stable across runs and safe to reference in fixture paths.
	wsID := seedWorkspace(h)

	// Register a fake agent so /api/agents returns a populated list and
	// /api/sessions create has a target (it will still fail to launch the
	// agent binary, which is itself a contract-relevant 400).
	seedAgent(h)

	cases := buildRESTCases(wsID)
	for _, c := range cases {
		fix, err := runRESTCase(h, c)
		if err != nil {
			return fmt.Errorf("rest case %s: %w", c.name, err)
		}
		// Marshal then redact the FULL fixture text so the envelope's request
		// path (which may carry the workspace ID) is scrubbed alongside the
		// body. This keeps golden files byte-stable across runs.
		data, err := json.MarshalIndent(fix, "", "  ")
		if err != nil {
			return fmt.Errorf("marshal %s: %w", c.name, err)
		}
		redacted := h.redactor.String(string(data))
		path := filepath.Join(outDir, c.name+".json")
		if err := os.WriteFile(path, []byte(redacted+"\n"), 0o644); err != nil {
			return fmt.Errorf("write %s: %w", c.name, err)
		}
	}
	return nil
}

// buildRESTCases enumerates every supported REST route with at least a success
// case and a relevant failure case. wsID is the seeded workspace ID used in
// workspace-scoped paths.
func buildRESTCases(wsID string) []restCase {
	var cases []restCase

	// --- Health (no auth) ---
	cases = append(cases,
		restCase{name: "health_ok", method: http.MethodGet, path: "/health", loopback: true},
	)

	// --- Pairing (no auth, rate-limited) ---
	cases = append(cases,
		restCase{name: "pair_initiate_ok", method: http.MethodPost, path: "/api/pair/initiate", body: `{"host":"localhost","port":7337}`, loopback: true},
		restCase{name: "pair_initiate_bad_body", method: http.MethodPost, path: "/api/pair/initiate", body: `{not json`, loopback: true},
		restCase{name: "pair_verify_passcode_bad_body", method: http.MethodPost, path: "/api/pair/verify-passcode", body: `{not json`, loopback: true},
		restCase{name: "pair_verify_passcode_wrong", method: http.MethodPost, path: "/api/pair/verify-passcode", body: `{"passcode":"wrong wrong wrong wrong","deviceName":"dev"}`, loopback: true},
		restCase{name: "pair_verify_token_wrong", method: http.MethodPost, path: "/api/pair/verify-token", body: `{"token":"deadbeef","deviceName":"dev"}`, loopback: true},
	)

	// --- Devices (auth) ---
	cases = append(cases,
		restCase{name: "devices_list_ok", method: http.MethodGet, path: "/api/devices", loopback: true},
		restCase{name: "devices_list_unauth", method: http.MethodGet, path: "/api/devices", loopback: false},
		restCase{name: "devices_revoke_not_found", method: http.MethodDelete, path: "/api/devices/nonexistent", loopback: true},
		restCase{name: "devices_cancel_revocation_bad_body", method: http.MethodPost, path: "/api/devices/cancel-revocation", body: `{not json`, loopback: true},
	)

	// --- Pending actions ---
	cases = append(cases,
		restCase{name: "pending_actions_list_ok", method: http.MethodGet, path: "/api/pending-actions", loopback: true},
		restCase{name: "pending_actions_list_unauth", method: http.MethodGet, path: "/api/pending-actions", loopback: false},
		restCase{name: "workspaces_cancel_registration_bad_body", method: http.MethodPost, path: "/api/workspaces/cancel-registration", body: `{not json`, loopback: true},
	)

	// --- Workspaces (auth) ---
	cases = append(cases,
		restCase{name: "workspaces_list_ok", method: http.MethodGet, path: "/api/workspaces", loopback: true},
		restCase{name: "workspaces_register_remote_disabled", method: http.MethodPost, path: "/api/workspaces", body: `{"path":"/tmp"}`, loopback: true},
		restCase{name: "workspaces_register_bad_body", method: http.MethodPost, path: "/api/workspaces", body: `{not json`, loopback: true},
		restCase{name: "workspaces_files_ok", method: http.MethodGet, path: fmt.Sprintf("/api/workspaces/%s/files", wsID), loopback: true},
		restCase{name: "workspaces_files_not_found", method: http.MethodGet, path: "/api/workspaces/nonexistent/files", loopback: true},
		restCase{name: "workspaces_read_ok", method: http.MethodGet, path: fmt.Sprintf("/api/workspaces/%s/file?path=README.md", wsID), loopback: true},
		restCase{name: "workspaces_read_missing_path", method: http.MethodGet, path: fmt.Sprintf("/api/workspaces/%s/file", wsID), loopback: true},
		restCase{name: "workspaces_read_not_found", method: http.MethodGet, path: fmt.Sprintf("/api/workspaces/%s/file?path=nope.txt", wsID), loopback: true},
		restCase{name: "workspaces_raw_ok", method: http.MethodGet, path: fmt.Sprintf("/api/workspaces/%s/raw?path=README.md", wsID), loopback: true},
		restCase{name: "workspaces_search_ok", method: http.MethodGet, path: fmt.Sprintf("/api/workspaces/%s/search?q=hello", wsID), loopback: true},
		restCase{name: "workspaces_write_bad_body", method: http.MethodPost, path: fmt.Sprintf("/api/workspaces/%s/file", wsID), body: `{not json`, loopback: true},
	)

	// --- Events ---
	cases = append(cases,
		restCase{name: "events_list_ok", method: http.MethodGet, path: "/api/events", loopback: true},
		restCase{name: "events_list_unauth", method: http.MethodGet, path: "/api/events", loopback: false},
		restCase{name: "events_session_ok", method: http.MethodGet, path: "/api/events/nonexistent", loopback: true},
	)

	// --- Agents ---
	cases = append(cases,
		restCase{name: "agents_list_ok", method: http.MethodGet, path: "/api/agents", loopback: true},
		restCase{name: "agents_upsert_bad_body", method: http.MethodPost, path: "/api/agents", body: `{not json`, loopback: true},
		restCase{name: "agents_delete_ok", method: http.MethodDelete, path: "/api/agents/fixture-agent", loopback: true},
		restCase{name: "agents_autodetect_ok", method: http.MethodPost, path: "/api/agents/autodetect", loopback: true},
	)

	// --- Sessions ---
	cases = append(cases,
		restCase{name: "sessions_list_ok", method: http.MethodGet, path: "/api/sessions", loopback: true},
		restCase{name: "sessions_get_not_found", method: http.MethodGet, path: "/api/sessions/nonexistent", loopback: true},
		restCase{name: "sessions_export_not_found", method: http.MethodGet, path: "/api/sessions/nonexistent/export", loopback: true},
		restCase{name: "sessions_create_bad_body", method: http.MethodPost, path: "/api/sessions", body: `{not json`, loopback: true},
		restCase{name: "sessions_create_unknown_agent", method: http.MethodPost, path: "/api/sessions", body: `{"agentId":"no-such-agent","modelId":"m","workspaceId":""}`, loopback: true},
		restCase{name: "sessions_patch_bad_body", method: http.MethodPatch, path: "/api/sessions/nonexistent", body: `{not json`, loopback: true},
		restCase{name: "sessions_prompt_not_found", method: http.MethodPost, path: "/api/sessions/nonexistent/prompt", body: `{"content":"hi"}`, loopback: true},
		restCase{name: "sessions_cancel_not_found", method: http.MethodPost, path: "/api/sessions/nonexistent/cancel", loopback: true},
		restCase{name: "sessions_close_not_found", method: http.MethodDelete, path: "/api/sessions/nonexistent", loopback: true},
		restCase{name: "sessions_context_bad_body", method: http.MethodPost, path: "/api/sessions/nonexistent/context", body: `{not json`, loopback: true},
		restCase{name: "sessions_providers_not_found", method: http.MethodGet, path: "/api/sessions/nonexistent/providers", loopback: true},
	)

	// --- Permissions ---
	cases = append(cases,
		restCase{name: "permissions_pending_ok", method: http.MethodGet, path: "/api/permissions/pending", loopback: true},
		restCase{name: "permissions_respond_bad_body", method: http.MethodPost, path: "/api/permissions/nonexistent/respond", body: `{not json`, loopback: true},
	)

	// --- MCP ---
	cases = append(cases,
		restCase{name: "mcp_get_ok", method: http.MethodGet, path: "/api/mcp", loopback: true},
		restCase{name: "mcp_put_bad_body", method: http.MethodPut, path: "/api/mcp", body: `{not json`, loopback: true},
		restCase{name: "mcp_patch_server_bad_body", method: http.MethodPatch, path: "/api/mcp/servers/fixture", body: `{not json`, loopback: true},
		restCase{name: "mcp_status_ok", method: http.MethodGet, path: "/api/mcp/status", loopback: true},
	)

	return cases
}

// runRESTCase executes one restCase against the harness server and returns the
// redacted fixture. It also registers any newly-revealed secrets (e.g. the
// pairing token from /api/pair/initiate) with the harness redactor so later
// captures scrub them.
func runRESTCase(h *harness, c restCase) (restFixture, error) {
	var bodyReader io.Reader
	if c.body != "" {
		bodyReader = strings.NewReader(c.body)
	}
	req := httptest.NewRequest(c.method, c.path, bodyReader)
	if c.loopback {
		req.RemoteAddr = "127.0.0.1:1234"
	} else {
		req.RemoteAddr = "10.0.0.7:1234"
	}
	if c.authHeader != "" {
		req.Header.Set("Authorization", c.authHeader)
	}
	if c.origin != "" {
		req.Header.Set("Origin", c.origin)
	}
	if c.body != "" {
		req.Header.Set("Content-Type", "application/json")
	}

	rec := httptest.NewRecorder()
	h.server.Handler().ServeHTTP(rec, req)

	body := rec.Body.String()
	// Register secrets revealed by pairing responses so subsequent captures
	// (and the WS/CLI captures) scrub them. The pairing session JSON contains
	// "token" and "passcode" fields.
	if strings.Contains(c.path, "/api/pair/") {
		registerPairingSecrets(h, body)
	}
	// Defense-in-depth: scrub any unregistered token/secret/secretHash fields.
	body = ScrubUnregisteredTokens(body)
	body = h.redactor.String(body)

	return restFixture{
		Method:      c.method,
		Path:        c.path,
		Status:      rec.Code,
		ContentType: rec.Header().Get("Content-Type"),
		Body:        body,
	}, nil
}

// registerPairingSecrets parses a pairing response body and registers the
// token and passcode values with the redactor. Failures are non-fatal: a
// malformed body simply means no secrets are registered.
func registerPairingSecrets(h *harness, body string) {
	var s struct {
		Token    string `json:"token"`
		Passcode string `json:"passcode"`
	}
	if err := json.Unmarshal([]byte(body), &s); err != nil {
		return
	}
	if s.Token != "" {
		h.redactor.RegisterSecret(s.Token, "<REDACTED_TOKEN>")
	}
	if s.Passcode != "" {
		h.redactor.RegisterSecret(s.Passcode, "<REDACTED_PASSCODE>")
	}
}
