package shell

import (
	"context"
	"os"
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

// TestRunAsyncArgsEnv verifies that env variables supplied via WithEnv are
// visible to the spawned process and take precedence over the inherited
// environment.
func TestRunAsyncArgsEnv(t *testing.T) {
	dir := t.TempDir()

	// Overlay a custom var on top of the inherited environment so the process
	// still has PATH etc.
	env := MergeEnv(os.Environ(), []string{"ACP_TEST_VAR=from-agent"})

	executor := NewExecutor(dir).WithEnv(env)

	var command string
	if runtime.GOOS == "windows" {
		command = "cmd"
	} else {
		command = "sh"
	}
	var args []string
	if runtime.GOOS == "windows" {
		args = []string{"/C", "echo %ACP_TEST_VAR%"}
	} else {
		args = []string{"-c", "printf %s \"$ACP_TEST_VAR\""}
	}

	result, err := executor.RunAsyncArgs(context.Background(), command, args, nil, nil)
	if err != nil {
		t.Fatalf("run: %v", err)
	}
	if result.ExitCode != 0 {
		t.Fatalf("exit code %d: %s", result.ExitCode, result.Stderr)
	}
	if got := strings.TrimSpace(result.Stdout); got != "from-agent" {
		t.Errorf("expected ACP_TEST_VAR=from-agent in subprocess env, got stdout %q", result.Stdout)
	}
}

// TestMergeEnv verifies that MergeEnv overlays extra on base with extra winning
// for duplicate keys, and preserves order for new keys.
func TestMergeEnv(t *testing.T) {
	base := []string{"PATH=/usr/bin", "HOME=/home/user", "EDITOR=vi"}
	extra := []string{"EDITOR=nano", "FOO=bar"}

	got := MergeEnv(base, extra)

	want := []string{"PATH=/usr/bin", "HOME=/home/user", "EDITOR=nano", "FOO=bar"}
	if len(got) != len(want) {
		t.Fatalf("got %v, want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("index %d: got %q, want %q", i, got[i], want[i])
		}
	}
}

// TestMergeEnvEmptyBase verifies that merging onto an empty base just yields extra.
func TestMergeEnvEmptyBase(t *testing.T) {
	got := MergeEnv(nil, []string{"A=1", "B=2"})
	if len(got) != 2 || got[0] != "A=1" || got[1] != "B=2" {
		t.Errorf("got %v, want [A=1 B=2]", got)
	}
}
