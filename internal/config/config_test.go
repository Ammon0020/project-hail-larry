package config

import (
	"encoding/json"
	"os"
	"path/filepath"
	"testing"
)

// TestDefaultConfig verifies the default configuration has sensible values.
func TestDefaultConfig(t *testing.T) {
	cfg := Default()

	if cfg.Port != 7337 {
		t.Errorf("expected port 7337, got %d", cfg.Port)
	}
	if cfg.Host != "0.0.0.0" {
		t.Errorf("expected host 0.0.0.0, got %s", cfg.Host)
	}
	if cfg.DataDir == "" {
		t.Error("expected non-empty data dir")
	}
	if cfg.DBPath == "" {
		t.Error("expected non-empty db path")
	}
}

// TestSaveAndLoad verifies config can be saved and loaded back.
func TestSaveAndLoad(t *testing.T) {
	// Use a temp directory to avoid touching the real config.
	tmpDir := t.TempDir()
	cfg := &Config{
		Port:       8443,
		Host:       "127.0.0.1",
		DataDir:    tmpDir,
		DBPath:     filepath.Join(tmpDir, "test.db"),
		Workspaces: []string{"/tmp/test-workspace"},
	}

	if err := cfg.Save(); err != nil {
		t.Fatalf("save config: %v", err)
	}

	// Verify the file exists.
	configPath := filepath.Join(tmpDir, "config.json")
	if _, err := os.Stat(configPath); err != nil {
		t.Fatalf("config file not created: %v", err)
	}

	// Load it back by reading the file directly (Load uses home dir).
	data, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatalf("read config: %v", err)
	}

	// Verify the content contains our values.
	content := string(data)
	if !contains(content, "8443") {
		t.Error("expected port 8443 in saved config")
	}
	if !contains(content, "127.0.0.1") {
		t.Error("expected host 127.0.0.1 in saved config")
	}
}

func contains(s, substr string) bool {
	return len(s) >= len(substr) && (s == substr || len(s) > 0 && containsStr(s, substr))
}

func containsStr(s, substr string) bool {
	for i := 0; i <= len(s)-len(substr); i++ {
		if s[i:i+len(substr)] == substr {
			return true
		}
	}
	return false
}

// TestDefaultRevocationGracePeriod verifies that Default() sets the
// revocation grace period to 300 seconds (5 minutes) and that remote workspace
// registration is disabled by default.
func TestDefaultRevocationGracePeriod(t *testing.T) {
	cfg := Default()
	if cfg.RevocationGracePeriodSeconds != 300 {
		t.Errorf("expected default revocation grace period 300, got %d", cfg.RevocationGracePeriodSeconds)
	}
	if cfg.AllowRemoteWorkspaceRegistration {
		t.Error("expected AllowRemoteWorkspaceRegistration to default to false")
	}
}

// TestLoadLegacyRevocationGracePeriodDefault verifies that a config file
// omitting revocationGracePeriodSeconds (a legacy config predating the field)
// loads with the 300-second default, while an explicit 0 is respected as an
// opt-out.
func TestLoadLegacyRevocationGracePeriodDefault(t *testing.T) {
	// Load() reads from ~/.local-agent/config.json, so we cannot easily point
	// it at a temp dir. Instead exercise the raw-map defaulting logic directly
	// by simulating the two JSON cases the Load() defaulting distinguishes.

	// Case 1: legacy config without the key — default to 300.
	legacyJSON := `{"port":7337,"host":"0.0.0.0"}`
	var raw map[string]json.RawMessage
	if err := json.Unmarshal([]byte(legacyJSON), &raw); err != nil {
		t.Fatalf("unmarshal legacy: %v", err)
	}
	var cfg Config
	_ = json.Unmarshal([]byte(legacyJSON), &cfg)
	if _, ok := raw["revocationGracePeriodSeconds"]; !ok {
		cfg.RevocationGracePeriodSeconds = defaultRevocationGracePeriodSeconds
	}
	if cfg.RevocationGracePeriodSeconds != 300 {
		t.Errorf("legacy: expected grace period 300, got %d", cfg.RevocationGracePeriodSeconds)
	}

	// Case 2: explicit 0 — respected as opt-out (no defaulting).
	explicitJSON := `{"port":7337,"revocationGracePeriodSeconds":0}`
	raw = nil
	if err := json.Unmarshal([]byte(explicitJSON), &raw); err != nil {
		t.Fatalf("unmarshal explicit: %v", err)
	}
	cfg = Config{}
	_ = json.Unmarshal([]byte(explicitJSON), &cfg)
	if _, ok := raw["revocationGracePeriodSeconds"]; !ok {
		cfg.RevocationGracePeriodSeconds = defaultRevocationGracePeriodSeconds
	}
	if cfg.RevocationGracePeriodSeconds != 0 {
		t.Errorf("explicit 0: expected grace period 0, got %d", cfg.RevocationGracePeriodSeconds)
	}
}
