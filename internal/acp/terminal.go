package acp

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
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

// CreateTerminal starts a command in the session workspace and returns a terminal
// id immediately. Output is streamed via shell events and buffered for later
// retrieval through TerminalOutput / WaitForTerminalExit.
func (c *acpClientImpl) CreateTerminal(ctx context.Context, params acp.CreateTerminalRequest) (acp.CreateTerminalResponse, error) {
	cwd := c.workspacePath
	if params.Cwd != nil && *params.Cwd != "" {
		cwd = *params.Cwd
	}

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
		res, runErr := executor.RunAsync(runCtx, cmdStr, onOutput, onOutput)
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

// genTerminalID returns a unique terminal identifier.
func genTerminalID() (string, error) {
	b := make([]byte, 8)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return "term-" + hex.EncodeToString(b), nil
}
