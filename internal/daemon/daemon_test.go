package daemon

import (
	"os"
	"path/filepath"
	"strconv"
	"testing"

	"github.com/adama/local-agent/internal/acp"
)

// TestDefaultConfig verifies the default configuration has sensible values.
func TestDefaultConfig(t *testing.T) {
	cfg := DefaultConfig()

	if cfg.Port != 7337 {
		t.Errorf("expected port 7337, got %d", cfg.Port)
	}
	if cfg.Host != "0.0.0.0" {
		t.Errorf("expected host 0.0.0.0, got %s", cfg.Host)
	}
	if cfg.DataDir == "" {
		t.Error("expected non-empty data dir")
	}
}

// TestIsRunningNoPidFile verifies IsRunning returns 0 when no PID file exists.
func TestIsRunningNoPidFile(t *testing.T) {
	tmpDir := t.TempDir()

	pid, err := IsRunning(tmpDir)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if pid != 0 {
		t.Errorf("expected pid 0 when no pid file, got %d", pid)
	}
}

// TestIsRunningStalePidFile verifies stale PID files are cleaned up.
func TestIsRunningStalePidFile(t *testing.T) {
	tmpDir := t.TempDir()

	// Write a PID file with a non-existent PID (999999 should not exist).
	pidFile := filepath.Join(tmpDir, "daemon.pid")
	if err := os.WriteFile(pidFile, []byte("999999"), 0644); err != nil {
		t.Fatalf("write pid file: %v", err)
	}

	pid, err := IsRunning(tmpDir)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if pid != 0 {
		t.Errorf("expected pid 0 for stale pid file, got %d", pid)
	}

	// Verify the stale PID file was cleaned up.
	if _, err := os.Stat(pidFile); !os.IsNotExist(err) {
		t.Error("expected stale pid file to be removed")
	}
}

// TestIsRunningCurrentProcess verifies IsRunning detects a live process.
func TestIsRunningCurrentProcess(t *testing.T) {
	tmpDir := t.TempDir()

	// Write a PID file with the current process's PID.
	pidFile := filepath.Join(tmpDir, "daemon.pid")
	currentPid := os.Getpid()
	if err := os.WriteFile(pidFile, []byte(strconv.Itoa(currentPid)), 0644); err != nil {
		t.Fatalf("write pid file: %v", err)
	}

	pid, err := IsRunning(tmpDir)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if pid != currentPid {
		t.Errorf("expected pid %d, got %d", currentPid, pid)
	}
}

// TestStopNotRunning verifies Stop returns an error when daemon is not running.
func TestStopNotRunning(t *testing.T) {
	tmpDir := t.TempDir()

	err := Stop(tmpDir)
	if err == nil {
		t.Error("expected error when stopping non-running daemon")
	}
}

func TestMergeAutodetectedAgentsPreservesConfiguredAgent(t *testing.T) {
	configured := []acp.AgentInfo{
		{
			ID:      "codex",
			Name:    "My Codex",
			Command: `C:\custom\codex.exe`,
			Models:  []acp.AgentModel{{ID: "custom-model", Name: "Custom Model"}},
			Warning: "manual warning",
		},
	}
	detected := []acp.AgentInfo{
		{
			ID:      "codex",
			Name:    "Codex CLI",
			Command: `C:\path\codex.exe`,
			Models:  []acp.AgentModel{{ID: "detected-model", Name: "Detected Model"}},
			Warning: "detected warning",
		},
	}

	merged, changed := mergeAutodetectedAgents(configured, detected)
	if changed {
		t.Fatal("expected unchanged config when detected agent matches fully configured agent")
	}
	if got := merged[0].Command; got != configured[0].Command {
		t.Fatalf("command was overwritten: got %q, want %q", got, configured[0].Command)
	}
	if got := merged[0].Models[0].ID; got != "custom-model" {
		t.Fatalf("models were overwritten: got %q", got)
	}
	if got := merged[0].Warning; got != "manual warning" {
		t.Fatalf("warning was overwritten: got %q", got)
	}
}

func TestMergeAutodetectedAgentsFillsEmptyConfiguredFields(t *testing.T) {
	configured := []acp.AgentInfo{{ID: "codex"}}
	detected := []acp.AgentInfo{
		{
			ID:      "codex",
			Name:    "Codex CLI",
			Command: `C:\path\codex.exe`,
			Models:  []acp.AgentModel{{ID: "detected-model", Name: "Detected Model"}},
			Warning: "detected warning",
		},
	}

	merged, changed := mergeAutodetectedAgents(configured, detected)
	if !changed {
		t.Fatal("expected config change when filling empty configured fields")
	}
	if got := merged[0].Name; got != "Codex CLI" {
		t.Fatalf("name not filled: got %q", got)
	}
	if got := merged[0].Command; got != `C:\path\codex.exe` {
		t.Fatalf("command not filled: got %q", got)
	}
	if got := merged[0].Models[0].ID; got != "detected-model" {
		t.Fatalf("models not filled: got %q", got)
	}
	if got := merged[0].Warning; got != "detected warning" {
		t.Fatalf("warning not filled: got %q", got)
	}
}

func TestMergeAutodetectedAgentsAddsNewAgent(t *testing.T) {
	detected := []acp.AgentInfo{{ID: "mistral-vibe", Name: "Mistral Vibe", Command: "vibe-acp"}}

	merged, changed := mergeAutodetectedAgents(nil, detected)
	if !changed {
		t.Fatal("expected config change when adding detected agent")
	}
	if len(merged) != 1 {
		t.Fatalf("expected one merged agent, got %d", len(merged))
	}
	if merged[0].ID != "mistral-vibe" {
		t.Fatalf("unexpected merged agent ID %q", merged[0].ID)
	}
}
