package acp

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"path/filepath"
	"strings"
	"sync"
	"unicode/utf8"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/shell"
	"github.com/coder/acp-go-sdk"
)

// defaultTerminalOutputLimit caps retained terminal output when the agent does
// not specify an OutputByteLimit (1 MiB).
const defaultTerminalOutputLimit = 1 << 20

// terminalEntry tracks a single agent-requested terminal: its command, captured
// output (capped, truncated from the front), and exit status once finished.
type terminalEntry struct {
	mu        sync.Mutex
	command   string
	output    []byte
	limit     int
	truncated bool
	exit      *acp.TerminalExitStatus
	done      chan struct{}
	cancel    context.CancelFunc
}

// appendOutput appends s to the retained output, truncating from the front (on a
// UTF-8 rune boundary) when the byte limit is exceeded.
func (e *terminalEntry) appendOutput(s string) {
	e.mu.Lock()
	defer e.mu.Unlock()
	e.output = append(e.output, s...)
	if e.limit > 0 && len(e.output) > e.limit {
		over := len(e.output) - e.limit
		// Advance past any partial rune so the retained output stays valid.
		for over < len(e.output) && !utf8.RuneStart(e.output[over]) {
			over++
		}
		e.output = e.output[over:]
		e.truncated = true
	}
}

// snapshot returns a copy of the current output and exit status.
func (e *terminalEntry) snapshot() (string, bool, *acp.TerminalExitStatus) {
	e.mu.Lock()
	defer e.mu.Unlock()
	return string(e.output), e.truncated, e.exit
}

// emit forwards an event to the registered callbacks, if any.
func (c *acpClientImpl) emit(ev interfaces.Event) {
	if c.callbacks != nil {
		c.callbacks.OnEvent(ev)
	}
}

// getTerminal returns the terminal entry for id, or nil if unknown.
func (c *acpClientImpl) getTerminal(id string) *terminalEntry {
	c.termMu.Lock()
	defer c.termMu.Unlock()
	return c.terminals[id]
}

// resolveCwd validates an agent-supplied working directory against the workspace
// root and returns a safe cwd. If candidate is empty or resolves outside the
// workspace (via a filepath.Rel containment check), the workspace root is used
// instead. This prevents an agent from escaping the workspace boundary by
// requesting a terminal with Cwd set to e.g. ~/.ssh, /etc, or C:\Windows.
//
// The containment check mirrors the safeJoin-style logic used by the workspace
// manager for file reads/writes: the relative path from the workspace root to
// the cleaned candidate must not start with ".." and must not be absolute.
func resolveCwd(workspacePath, candidate string) string {
	if candidate == "" {
		return workspacePath
	}
	cleaned := filepath.Clean(candidate)
	// An absolute candidate is only accepted if it is inside the workspace.
	rel, err := filepath.Rel(workspacePath, cleaned)
	if err != nil {
		return workspacePath
	}
	if rel == "." {
		// Candidate is the workspace root itself.
		return workspacePath
	}
	if strings.HasPrefix(rel, "..") || filepath.IsAbs(rel) {
		// Resolves outside the workspace — fall back to the safe root.
		return workspacePath
	}
	return cleaned
}

// CreateTerminal starts a command in the session workspace and returns a terminal
// id immediately. Output is streamed via shell events and buffered for later
// retrieval through TerminalOutput / WaitForTerminalExit.
//
// The agent may supply a Cwd, but it is validated against the workspace root
// (see resolveCwd); any path that resolves outside the workspace is rejected
// and the command runs in the workspace root instead. The resolved cwd is
// surfaced in the EventShellCommandStarted event so the user can see where the
// command will execute.
func (c *acpClientImpl) CreateTerminal(ctx context.Context, params acp.CreateTerminalRequest) (acp.CreateTerminalResponse, error) {
	cwd := c.workspacePath
	if params.Cwd != nil && *params.Cwd != "" {
		cwd = resolveCwd(c.workspacePath, *params.Cwd)
	}

	// Build a human-readable command string for events and the terminal entry.
	// This is for display only; the actual execution uses the structured
	// argument list (command + args) passed directly to exec.Command via
	// RunAsyncArgs, so args containing spaces or shell metacharacters are not
	// re-parsed by a shell.
	cmdStr := params.Command
	for _, a := range params.Args {
		cmdStr += " " + a
	}

	limit := defaultTerminalOutputLimit
	if params.OutputByteLimit != nil && *params.OutputByteLimit > 0 {
		limit = *params.OutputByteLimit
	}

	id, err := genTerminalID()
	if err != nil {
		return acp.CreateTerminalResponse{}, fmt.Errorf("generate terminal id: %w", err)
	}

	entry := &terminalEntry{command: cmdStr, limit: limit, done: make(chan struct{})}
	// Detach from the request context (which ends when this RPC returns) but
	// allow Kill/Release to cancel the process.
	runCtx, cancel := context.WithCancel(context.WithoutCancel(ctx))
	entry.cancel = cancel

	c.termMu.Lock()
	if c.terminals == nil {
		c.terminals = make(map[string]*terminalEntry)
	}
	c.terminals[id] = entry
	c.termMu.Unlock()

	c.emit(interfaces.Event{
		Type:      interfaces.EventShellCommandStarted,
		SessionID: c.sessionID,
		Command:   cmdStr,
		Cwd:       cwd,
	})

	executor := shell.NewExecutor(cwd)
	go func() {
		onOutput := func(s string) {
			entry.appendOutput(s)
			c.emit(interfaces.Event{
				Type:      interfaces.EventShellOutputStreamed,
				SessionID: c.sessionID,
				Content:   s,
			})
		}
		// Pass the structured command + args directly to exec.Command (no
		// shell wrapping) so arguments containing spaces or shell
		// metacharacters are passed verbatim instead of being re-parsed.
		res, runErr := executor.RunAsyncArgs(runCtx, params.Command, params.Args, onOutput, onOutput)
		code := res.ExitCode
		entry.mu.Lock()
		entry.exit = &acp.TerminalExitStatus{ExitCode: &code}
		entry.mu.Unlock()
		close(entry.done)

		summary := res.Stderr
		if runErr != nil {
			summary = runErr.Error()
		}
		c.emit(interfaces.Event{
			Type:      interfaces.EventShellCommandCompleted,
			SessionID: c.sessionID,
			Command:   cmdStr,
			ExitCode:  &code,
			Summary:   summary,
		})
	}()

	return acp.CreateTerminalResponse{TerminalId: id}, nil
}

// TerminalOutput returns the buffered output and, if finished, the exit status.
func (c *acpClientImpl) TerminalOutput(_ context.Context, params acp.TerminalOutputRequest) (acp.TerminalOutputResponse, error) {
	entry := c.getTerminal(params.TerminalId)
	if entry == nil {
		return acp.TerminalOutputResponse{}, fmt.Errorf("terminal not found: %s", params.TerminalId)
	}
	output, truncated, exit := entry.snapshot()
	return acp.TerminalOutputResponse{
		Output:     output,
		Truncated:  truncated,
		ExitStatus: exit,
	}, nil
}

// WaitForTerminalExit blocks until the command finishes (or ctx is cancelled).
func (c *acpClientImpl) WaitForTerminalExit(ctx context.Context, params acp.WaitForTerminalExitRequest) (acp.WaitForTerminalExitResponse, error) {
	entry := c.getTerminal(params.TerminalId)
	if entry == nil {
		return acp.WaitForTerminalExitResponse{}, fmt.Errorf("terminal not found: %s", params.TerminalId)
	}
	select {
	case <-entry.done:
	case <-ctx.Done():
		return acp.WaitForTerminalExitResponse{}, ctx.Err()
	}
	_, _, exit := entry.snapshot()
	resp := acp.WaitForTerminalExitResponse{}
	if exit != nil {
		resp.ExitCode = exit.ExitCode
		resp.Signal = exit.Signal
	}
	return resp, nil
}

// KillTerminal terminates the process but keeps the terminal entry so its output
// remains retrievable.
func (c *acpClientImpl) KillTerminal(_ context.Context, params acp.KillTerminalRequest) (acp.KillTerminalResponse, error) {
	entry := c.getTerminal(params.TerminalId)
	if entry == nil {
		return acp.KillTerminalResponse{}, fmt.Errorf("terminal not found: %s", params.TerminalId)
	}
	if entry.cancel != nil {
		entry.cancel()
	}
	return acp.KillTerminalResponse{}, nil
}

// ReleaseTerminal kills the process (if running) and removes the terminal entry.
func (c *acpClientImpl) ReleaseTerminal(_ context.Context, params acp.ReleaseTerminalRequest) (acp.ReleaseTerminalResponse, error) {
	entry := c.getTerminal(params.TerminalId)
	if entry == nil {
		return acp.ReleaseTerminalResponse{}, fmt.Errorf("terminal not found: %s", params.TerminalId)
	}
	if entry.cancel != nil {
		entry.cancel()
	}
	c.termMu.Lock()
	delete(c.terminals, params.TerminalId)
	c.termMu.Unlock()
	return acp.ReleaseTerminalResponse{}, nil
}

// releaseAllTerminals cancels every outstanding terminal's run context and
// clears the terminal map. It is called on session close / daemon shutdown so
// that terminal subprocesses are killed and their reader goroutines exit
// instead of leaking. After this call every terminal's done channel will
// eventually close (once the cancelled RunAsyncArgs returns) and its output
// remains retrievable only if the caller held a reference to the entry; the map
// itself is emptied so no new lookups succeed.
func (c *acpClientImpl) releaseAllTerminals() {
	c.termMu.Lock()
	defer c.termMu.Unlock()
	for _, entry := range c.terminals {
		if entry.cancel != nil {
			entry.cancel()
		}
	}
	c.terminals = make(map[string]*terminalEntry)
}

// genTerminalID returns a unique terminal identifier.
func genTerminalID() (string, error) {
	b := make([]byte, 8)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return "term-" + hex.EncodeToString(b), nil
}
