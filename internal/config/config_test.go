package config

import (
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
