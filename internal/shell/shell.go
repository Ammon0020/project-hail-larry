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
	"sync"
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

	cmd := shellCommand(ctx, command)
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
			// Command ran but exited non-zero — this is a normal failure, not
			// an infrastructure error, so it is reported via ExitCode only.
			result.ExitCode = exitErr.ExitCode()
			return result, nil
		}
		// Command failed to start or the context was cancelled. Surface the
		// real error so callers can distinguish "could not run at all" from
		// "ran and exited non-zero".
		result.ExitCode = -1
		result.Stderr += "\n" + err.Error()
		return result, err
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

	cmd := shellCommand(ctx, command)
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

	if startErr := cmd.Start(); startErr != nil {
		return Result{}, fmt.Errorf("start command: %w", startErr)
	}

	// Read stdout and stderr in goroutines. A WaitGroup ensures both reader
	// goroutines finish writing to their buffers before we read them below —
	// without it, reading stdoutBuf/stderrBuf immediately after cmd.Wait()
	// races with the still-running reader goroutines.
	var stdoutBuf, stderrBuf bytes.Buffer
	var wg sync.WaitGroup
	wg.Add(2)
	go func() {
		defer wg.Done()
		readPipe(stdoutPipe, &stdoutBuf, onStdout)
	}()
	go func() {
		defer wg.Done()
		readPipe(stderrPipe, &stderrBuf, onStderr)
	}()

	err = cmd.Wait()
	// Wait for the reader goroutines to drain the pipes before reading the
	// buffers. cmd.Wait closes the pipes, causing readPipe to return, but the
	// final buffered writes may still be in flight without this barrier.
	wg.Wait()

	result := Result{
		Stdout:   stdoutBuf.String(),
		Stderr:   stderrBuf.String(),
		ExitCode: 0,
	}

	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			// Command ran but exited non-zero — report via ExitCode only.
			result.ExitCode = exitErr.ExitCode()
			return result, nil
		}
		// Command failed to start or the context was cancelled. Surface the
		// real error so callers can distinguish "could not run at all" from
		// "ran and exited non-zero".
		result.ExitCode = -1
		result.Stderr += "\n" + err.Error()
		return result, err
	}

	return result, nil
}

// shellCommand builds the OS-specific shell invocation for an approved command.
func shellCommand(ctx context.Context, command string) *exec.Cmd {
	if runtime.GOOS == "windows" {
		return exec.CommandContext(ctx, "cmd", "/C", command) //nolint:gosec // commands are executed only after client permission approval.
	}
	return exec.CommandContext(ctx, "sh", "-c", command) //nolint:gosec // commands are executed only after client permission approval.
}

// RunAsyncArgs executes a command with an explicit argument list (no shell
// wrapping) and streams output via the provided callbacks. Unlike RunAsync,
// which joins everything into a single string passed to `sh -c`/`cmd /C`, this
// method passes command and args directly to exec.Command, preserving the
// structured argument list. This avoids re-parsing by the shell: an argument
// containing spaces, quotes, or shell metacharacters (`;`, `|`, `$`, backticks)
// is passed verbatim to the child process instead of being re-interpreted.
//
// onStdout and onStderr are called incrementally as output is produced. Returns
// the final result when the command completes. A non-nil error is returned only
// when the command could not start or the context was cancelled (non-zero exit
// codes are reported via Result.ExitCode with a nil error).
func (e *Executor) RunAsyncArgs(ctx context.Context, command string, args []string, onStdout, onStderr func(string)) (Result, error) {
	if command == "" {
		return Result{}, fmt.Errorf("empty command")
	}

	cmd := exec.CommandContext(ctx, command, args...) //nolint:gosec // commands are executed only after client permission approval.
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

	if startErr := cmd.Start(); startErr != nil {
		return Result{}, fmt.Errorf("start command: %w", startErr)
	}

	// Read stdout and stderr in goroutines. A WaitGroup ensures both reader
	// goroutines finish writing to their buffers before we read them below —
	// without it, reading stdoutBuf/stderrBuf immediately after cmd.Wait()
	// races with the still-running reader goroutines.
	var stdoutBuf, stderrBuf bytes.Buffer
	var wg sync.WaitGroup
	wg.Add(2)
	go func() {
		defer wg.Done()
		readPipe(stdoutPipe, &stdoutBuf, onStdout)
	}()
	go func() {
		defer wg.Done()
		readPipe(stderrPipe, &stderrBuf, onStderr)
	}()

	err = cmd.Wait()
	// Wait for the reader goroutines to drain the pipes before reading the
	// buffers. cmd.Wait closes the pipes, causing readPipe to return, but the
	// final buffered writes may still be in flight without this barrier.
	wg.Wait()

	result := Result{
		Stdout:   stdoutBuf.String(),
		Stderr:   stderrBuf.String(),
		ExitCode: 0,
	}

	if err != nil {
		if exitErr, ok := err.(*exec.ExitError); ok {
			// Command ran but exited non-zero — report via ExitCode only.
			result.ExitCode = exitErr.ExitCode()
			return result, nil
		}
		// Command failed to start or the context was cancelled. Surface the
		// real error so callers can distinguish "could not run at all" from
		// "ran and exited non-zero".
		result.ExitCode = -1
		result.Stderr += "\n" + err.Error()
		return result, err
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
