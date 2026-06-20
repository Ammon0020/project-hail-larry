// Package shell implements workspace-scoped shell execution.
// Blueprint references: Sec 15 (Shell Execution).
//
// The daemon executes approved shell commands on behalf of agents via ACP.
// Commands run within workspace boundaries. Output is streamed as events.
package shell

import (
	"bytes"
	"context"
	"fmt"
	"os/exec"
	"runtime"
)

// Result holds the output and exit code of a completed command.
type Result struct {
	Stdout   string `json:"stdout"`
	Stderr   string `json:"stderr"`
	ExitCode int    `json:"exitCode"`
}

// Executor runs shell commands within a workspace directory.
type Executor struct {
	workspacePath string
}

// NewExecutor creates a new shell Executor scoped to the given workspace path.
func NewExecutor(workspacePath string) *Executor {
	return &Executor{workspacePath: workspacePath}
}

// Run executes a command in the workspace directory and returns the result.
// The command runs with a timeout from the context. Output is captured fully
// (streaming will be added when the event system is wired in Phase 1 integration).
func (e *Executor) Run(ctx context.Context, command string) (Result, error) {
	if command == "" {
		return Result{}, fmt.Errorf("empty command")
	}

	var cmd *exec.Cmd
	if runtime.GOOS == "windows" {
		cmd = exec.CommandContext(ctx, "cmd", "/C", command)
	} else {
		cmd = exec.CommandContext(ctx, "sh", "-c", command)
	}

	// Set the working directory to the workspace path.
	cmd.Dir = e.workspacePath

	var stdout, stderr bytes.Buffer
	cmd.Stdout = &stdout
	cmd.Stderr = &stderr

	err := cmd.Run()

	result := Result{
		Stdout:   stdout.String(),
		Stderr:   stderr.String(),
		ExitCode: 0,
	}

	if err != nil {
		// Try to extract the exit code.
		if exitErr, ok := err.(*exec.ExitError); ok {
			result.ExitCode = exitErr.ExitCode()
		} else {
			// Command failed to start or context was cancelled.
			result.ExitCode = -1
			result.Stderr += "\n" + err.Error()
		}
	}

	return result, nil
}

// RunAsync executes a command and streams output via the provided callbacks.
// onStdout and onStderr are called incrementally as output is produced.
// Returns the final result when the command completes.
func (e *Executor) RunAsync(ctx context.Context, command string, onStdout, onStderr func(string)) (Result, error) {
	if command == "" {
		return Result{}, fmt.Errorf("empty command")
	}

	var cmd *exec.Cmd
	if runtime.GOOS == "windows" {
		cmd = exec.CommandContext(ctx, "cmd", "/C", command)
	} else {
		cmd = exec.CommandContext(ctx, "sh", "-c", command)
	}

	cmd.Dir = e.workspacePath

	// Get pipes for streaming.
	stdoutPipe, err := cmd.StdoutPipe()
	if err != nil {
		return Result{}, fmt.Errorf("stdout pipe: %w", err)
	}
	stderrPipe, err := cmd.StderrPipe()
	if err != nil {
		return Result{}, fmt.Errorf("stderr pipe: %w", err)
	}

	if err := cmd.Start(); err != nil {
		return Result{}, fmt.Errorf("start command: %w", err)
	}

	// Read stdout and stderr in goroutines.
	var stdoutBuf, stderrBuf bytes.Buffer

	go readPipe(stdoutPipe, &stdoutBuf, onStdout)
	go readPipe(stderrPipe, &stderrBuf, onStderr)

	err = cmd.Wait()

	result := Result{
		Stdout:   stdoutBuf.String(),
		Stderr:   stderrBuf.String(),
		ExitCode: 0,
	}

	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			result.ExitCode = exitErr.ExitCode()
		} else {
			result.ExitCode = -1
			result.Stderr += "\n" + err.Error()
		}
	}

	return result, nil
}

// readPipe reads from a pipe, writing to the buffer and calling the callback.
func readPipe(pipe interface{ Read([]byte) (int, error) }, buf *bytes.Buffer, callback func(string)) {
	buf2 := make([]byte, 4096)
	for {
		n, err := pipe.Read(buf2)
		if n > 0 {
			buf.Write(buf2[:n])
			if callback != nil {
				callback(string(buf2[:n]))
			}
		}
		if err != nil {
			return
		}
	}
}
