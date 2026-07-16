// Package main: seed config + workspace helpers.
//
// The harness writes a seed config.json into the isolated state dir before
// constructing the daemon so the daemon's startup load registers a known
// workspace and agent. This keeps the harness self-contained: the
// workspace-scoped REST routes have a real target with deterministic contents,
// and /api/agents returns a populated list, without depending on any real
// agent binary being installed.
package main

import (
	"encoding/json"
	"fmt"
	"net/http/httptest"
	"os"
	"path/filepath"
)

// seedWorkspaceRelPath is the path to the fixture workspace, relative to the
// repo root (the harness cwd when run via `go run ./tests/contract/...`).
const seedWorkspaceRelPath = "tests/contract/fixtures/seed-workspace"

// seedWorkspacePath returns the absolute path to the fixture workspace. It
// resolves the seed workspace relative to the repo root (so the path is
// correct whether the harness runs via `go run` from the repo root or via
// `go test` from the package dir) and verifies the directory exists so a
// misconfigured layout fails loudly instead of producing empty fixtures.
func seedWorkspacePath(repoRoot string) (string, error) {
	abs := filepath.Join(repoRoot, seedWorkspaceRelPath)
	if info, err := os.Stat(abs); err != nil || !info.IsDir() {
		return "", fmt.Errorf("seed workspace not found at %s", abs)
	}
	return abs, nil
}

// seedAgentJSON is the fake agent persisted in config.json so /api/agents
// returns a populated entry. Its command is intentionally bogus so the daemon
// marks it with a "not found" warning — that warning text is itself part of
// the contract surface and is captured in the agents_list_ok fixture.
const seedAgentJSON = `[
  {
    "id": "fixture-agent",
    "name": "Fixture Agent",
    "command": "fixture-agent-binary-not-on-path",
    "args": [],
    "models": [
      {"id": "fixture-model", "name": "Fixture Model"}
    ]
  }
]`

// writeSeedConfig writes a config.json into stateDir that registers the seed
// workspace and the fixture agent. The shape matches internal/config.Config
// JSON tags. The daemon's config.Load reads this on construction.
func writeSeedConfig(stateDir, seedWsPath string) error {
	// Build the config as a map so we control field order and omit empty
	// fields explicitly, then marshal indent for readability.
	cfg := map[string]any{
		"port":                          7337,
		"host":                          "127.0.0.1",
		"dataDir":                       stateDir,
		"dbPath":                        filepath.Join(stateDir, "local-agent.db"),
		"workspaces":                    []string{seedWsPath},
		"agents":                        json.RawMessage(seedAgentJSON),
		"tlsEnabled":                    false,
		"pairingTtlSeconds":             300,
		"revocationGracePeriodSeconds":  300,
		"credentialInactivityTtlSeconds": 2592000,
	}
	data, err := json.MarshalIndent(cfg, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal seed config: %w", err)
	}
	return os.WriteFile(filepath.Join(stateDir, "config.json"), data, 0o600)
}

// seedWorkspace returns the deterministic workspace ID for the seed workspace.
// The workspace manager derives the ID from a hash of the absolute path, so it
// is stable across runs as long as the seed workspace path is stable. The ID
// itself is machine-specific (different absolute path => different hash), so it
// is registered with the redactor and replaced with <REDACTED_WORKSPACE_ID> in
// all golden fixtures. This makes the fixtures portable: the future Rust
// differential runner registers its own workspace ID with the same placeholder.
//
// We compute it lazily by asking the loaded server's workspace manager via the
// /api/workspaces endpoint rather than re-implementing the hash, so the ID
// always matches whatever scheme the current Go code uses.
func seedWorkspace(h *harness) string {
	// Hit /api/workspaces via the in-process handler and parse the first
	// entry's ID. This guarantees the ID matches the daemon's derivation.
	// Use a raw (un-redacted) handler call so the ID is not scrubbed before we
	// can read it.
	req := httptest.NewRequest("GET", "/api/workspaces", nil)
	req.RemoteAddr = "127.0.0.1:1234"
	rec := httptest.NewRecorder()
	h.server.Handler().ServeHTTP(rec, req)
	if rec.Code != 200 {
		return "unknown-workspace-id"
	}
	var ws []struct {
		ID   string `json:"id"`
		Path string `json:"path"`
	}
	if err := json.Unmarshal(rec.Body.Bytes(), &ws); err != nil || len(ws) == 0 {
		return "unknown-workspace-id"
	}
	wsID := ws[0].ID
	// Register the workspace ID with the redactor so it is replaced with
	// <REDACTED_WORKSPACE_ID> in all captured fixtures (REST paths, response
	// bodies, CLI output). This makes golden fixtures portable across machines
	// with different repo paths (and therefore different workspace ID hashes).
	h.redactor.RegisterSecret(wsID, "<REDACTED_WORKSPACE_ID>")
	return wsID
}

// seedAgent is a no-op placeholder kept for symmetry with seedWorkspace. The
// fixture agent is seeded via config.json (see writeSeedConfig), so there is
// nothing to do at runtime; this function exists so captureREST can call a
// symmetric pair of seed helpers without conditional logic.
func seedAgent(h *harness) {}
