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
	"syscall"
)

// Result holds the output and exit status of a completed command.
type Result struct {
	Stdout   string  `json:"stdout"`
	Stderr   string  `json:"stderr"`
	ExitCode int     `json:"exitCode"`
	Signal   *string `json:"signal,omitempty"`
}

// Executor runs shell commands within a workspace directory.
type Executor struct {
	workspacePath string
	// env is the full "KEY=VALUE" environment for spawned processes. When nil,
	// the subprocess inherits the daemon's environment (os.Environ()). When set,
	// it fully replaces the inherited environment — callers should normally build
	// it as os.Environ() overlaid with their own vars (see MergeEnv).
	env []string
}

// NewExecutor creates a new shell Executor scoped to the given workspace path.
// The spawned process inherits the daemon environment.
func NewExecutor(workspacePath string) *Executor {
	return &Executor{workspacePath: workspacePath}
}

// WithEnv returns a copy of the executor with the environment set to env. env
// should be a "KEY=VALUE" slice (typically os.Environ() overlaid with
// caller-supplied variables via MergeEnv). A nil/empty env clears the
// inherited environment; pass os.Environ() explicitly to preserve it.
func (e *Executor) WithEnv(env []string) *Executor {
	return &Executor{workspacePath: e.workspacePath, env: env}
}

// MergeEnv overlays extra "KEY=VALUE" entries on top of base, with extra taking
// precedence for duplicate keys. The base slice is typically os.Environ() and
// extra is the agent-supplied environment. The returned slice has no duplicate
// keys.
func MergeEnv(base, extra []string) []string {
	seen := make(map[string]int, len(base)+len(extra))
	merged := make([]string, 0, len(base)+len(extra))
	// Insert base first, recording the index of each key so later duplicates
	// (from extra) can replace the existing entry in place.
	for _, kv := range base {
		k := envKey(kv)
		if idx, ok := seen[k]; ok {
			merged[idx] = kv
			continue
		}
		seen[k] = len(merged)
		merged = append(merged, kv)
	}
	for _, kv := range extra {
		k := envKey(kv)
		if idx, ok := seen[k]; ok {
			merged[idx] = kv
			continue
		}
		seen[k] = len(merged)
		merged = append(merged, kv)
	}
	return merged
}

// envKey returns the key portion of a "KEY=VALUE" environment string. Strings
// without '=' are treated as a key with an empty value (matching os/exec
// behavior, which ignores entries without '=').
func envKey(kv string) string {
	for i := 0; i < len(kv); i++ {
		if kv[i] == '=' {
			return kv[:i]
		}
	}
	return kv
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
	if e.env != nil {
		cmd.Env = e.env
	}

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
			// Command ran but exited non-zero or was killed by a signal.
			// Report the exit code and, on Unix, the terminating signal.
			result.ExitCode = exitErr.ExitCode()
			result.Signal = exitSignal(exitErr)
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
	if e.env != nil {
		cmd.Env = e.env
	}

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
			// Command ran but exited non-zero or was killed by a signal.
			result.ExitCode = exitErr.ExitCode()
			result.Signal = exitSignal(exitErr)
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
	if e.env != nil {
		cmd.Env = e.env
	}

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
			// Command ran but exited non-zero or was killed by a signal.
			result.ExitCode = exitErr.ExitCode()
			result.Signal = exitSignal(exitErr)
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

// exitSignal extracts the terminating signal from an exec.ExitError, if any.
func exitSignal(exitErr *exec.ExitError) *string {
	if exitErr == nil {
		return nil
	}
	ws, ok := exitErr.Sys().(syscall.WaitStatus)
	if !ok || !ws.Signaled() {
		return nil
	}
	s := ws.Signal().String()
	return &s
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
