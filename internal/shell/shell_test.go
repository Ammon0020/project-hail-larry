package shell

import (
	"context"
	"runtime"
	"strings"
	"testing"
)

// TestRunEcho verifies that a simple echo command runs and returns output.
func TestRunEcho(t *testing.T) {
	dir := t.TempDir()
	executor := NewExecutor(dir)

	result, err := executor.Run(context.Background(), "echo hello")
	if err != nil {
		t.Fatalf("run: %v", err)
	}

	if result.ExitCode != 0 {
		t.Errorf("expected exit code 0, got %d", result.ExitCode)
	}

	// Trim whitespace for comparison (echo adds newline on some platforms).
	output := strings.TrimSpace(result.Stdout)
	if output != "hello" {
		t.Errorf("expected stdout %q, got %q", "hello", output)
	}
}

// TestRunEmptyCommand verifies that an empty command returns an error.
func TestRunEmptyCommand(t *testing.T) {
	dir := t.TempDir()
	executor := NewExecutor(dir)

	_, err := executor.Run(context.Background(), "")
	if err == nil {
		t.Error("expected error for empty command")
	}
}

// TestRunExitCode verifies that non-zero exit codes are captured.
func TestRunExitCode(t *testing.T) {
	dir := t.TempDir()
	executor := NewExecutor(dir)

	var command string
	if runtime.GOOS == "windows" {
		command = "exit /b 1"
	} else {
		command = "exit 1"
	}

	result, _ := executor.Run(context.Background(), command)

	if result.ExitCode == 0 {
		t.Error("expected non-zero exit code")
	}
}

// TestRunWorkingDirectory verifies the command runs in the workspace directory.
func TestRunWorkingDirectory(t *testing.T) {
	dir := t.TempDir()
	executor := NewExecutor(dir)

	var command string
	if runtime.GOOS == "windows" {
		command = "cd"
	} else {
		command = "pwd"
	}

	result, err := executor.Run(context.Background(), command)
	if err != nil {
		t.Fatalf("run: %v", err)
	}

	output := strings.TrimSpace(result.Stdout)
	// The output should contain the workspace directory path.
	// On Windows, cd outputs the current directory.
	// On Unix, pwd outputs the absolute path.
	if !strings.Contains(output, dir) && !strings.Contains(strings.ToLower(output), strings.ToLower(dir)) {
		t.Errorf("expected output to contain %s, got %s", dir, output)
	}
}

// TestRunAsync verifies async execution with streaming callbacks.
func TestRunAsync(t *testing.T) {
	dir := t.TempDir()
	executor := NewExecutor(dir)

	var stdoutChunks []string
	result, err := executor.RunAsync(context.Background(), "echo streaming",
		func(s string) { stdoutChunks = append(stdoutChunks, s) },
		nil,
	)
	if err != nil {
		t.Fatalf("run async: %v", err)
	}

	if result.ExitCode != 0 {
		t.Errorf("expected exit code 0, got %d", result.ExitCode)
	}

	// Verify we got at least one stdout chunk via the callback.
	if len(stdoutChunks) == 0 {
		t.Error("expected at least one stdout chunk from callback")
	}
}
