package acp

import (
	"context"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"path/filepath"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/coder/acp-go-sdk"
)

// acpClientImpl implements the coder/acp-go-sdk Client interface.
type acpClientImpl struct {
	callbacks    interfaces.ACPCallbacks
	workspaceMgr interfaces.WorkspaceManager
	permMgr      interfaces.PermissionManager
	workspaceID  string
	sessionID    string // our internal session ID
}

func (c *acpClientImpl) SessionUpdate(ctx context.Context, params acp.SessionNotification) error {
	u := params.Update
	if c.callbacks == nil {
		return nil
	}

	switch {
	case u.AgentMessageChunk != nil:
		if u.AgentMessageChunk.Content.Text != nil {
			c.callbacks.OnEvent(interfaces.Event{
				Type:      interfaces.EventStreamUpdate,
				SessionID: c.sessionID,
				Role:      "agent",
				Content:   u.AgentMessageChunk.Content.Text.Text,
				Streaming: true,
			})
		}
	case u.ToolCall != nil:
		c.callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventToolStarted,
			SessionID: c.sessionID,
			Tool:      u.ToolCall.Title,
		})
	case u.ToolCallUpdate != nil:
		status := ""
		if u.ToolCallUpdate.Status != nil {
			status = string(*u.ToolCallUpdate.Status)
		}
		c.callbacks.OnEvent(interfaces.Event{
			Type:      interfaces.EventToolCompleted,
			SessionID: c.sessionID,
			Summary:   status,
		})
	case u.AgentThoughtChunk != nil:
		// Could emit thought updates here if desired
	case u.UserMessageChunk != nil:
		// Usually already emitted by our side
	}
	return nil
}

func (c *acpClientImpl) RequestPermission(ctx context.Context, params acp.RequestPermissionRequest) (acp.RequestPermissionResponse, error) {
	if c.permMgr == nil {
		return acp.RequestPermissionResponse{}, fmt.Errorf("permission manager not configured")
	}

	title := ""
	if params.ToolCall.Title != nil {
		title = *params.ToolCall.Title
	} else {
		title = string(params.ToolCall.ToolCallId)
	}

	opts := make([]interfaces.PermissionDecision, 0, len(params.Options))
	optMap := make(map[interfaces.PermissionDecision]string)

	for _, o := range params.Options {
		dec := interfaces.PermissionDecision(o.Kind)
		opts = append(opts, dec)
		optMap[dec] = string(o.OptionId)
	}

	req := interfaces.PermissionRequest{
		SessionID: c.sessionID,
		Tool:      title,
		Options:   opts,
	}

	decision, err := c.permMgr.Request(ctx, req)
	if err != nil {
		return acp.RequestPermissionResponse{}, err
	}

	optID, ok := optMap[decision]
	if !ok {
		return acp.RequestPermissionResponse{}, fmt.Errorf("unknown decision chosen: %s", decision)
	}

	return acp.RequestPermissionResponse{
		Outcome: acp.RequestPermissionOutcome{
			Selected: &acp.RequestPermissionOutcomeSelected{
				OptionId: acp.PermissionOptionId(optID),
			},
		},
	}, nil
}

func (c *acpClientImpl) ReadTextFile(ctx context.Context, params acp.ReadTextFileRequest) (acp.ReadTextFileResponse, error) {
	if c.workspaceMgr == nil {
		return acp.ReadTextFileResponse{}, fmt.Errorf("workspace manager not configured")
	}
	// Rel path assumes we are in the workspace root
	content, _, err := c.workspaceMgr.ReadFile(ctx, c.workspaceID, filepath.Clean(params.Path))
	if err != nil {
		return acp.ReadTextFileResponse{}, err
	}
	return acp.ReadTextFileResponse{Content: content}, nil
}

func (c *acpClientImpl) WriteTextFile(ctx context.Context, params acp.WriteTextFileRequest) (acp.WriteTextFileResponse, error) {
	if c.workspaceMgr == nil {
		return acp.WriteTextFileResponse{}, fmt.Errorf("workspace manager not configured")
	}
	// Agent writes don't use optimistic locking — pass expectedRevision=0.
	// The file-sync layer tracks revisions for user-facing edits; agent writes
	// are tracked via FileRevisionUpdated events emitted by the daemon.
	type fileWriter interface {
		WriteFile(ctx context.Context, workspaceID, relPath, content string, expectedRevision int64) (int64, error)
	}
	fw, ok := c.workspaceMgr.(fileWriter)
	if !ok {
		return acp.WriteTextFileResponse{}, fmt.Errorf("workspace manager does not support writing")
	}
	if _, err := fw.WriteFile(ctx, c.workspaceID, filepath.Clean(params.Path), params.Content, 0); err != nil {
		return acp.WriteTextFileResponse{}, err
	}
	return acp.WriteTextFileResponse{}, nil
}

func (c *acpClientImpl) CreateTerminal(ctx context.Context, params acp.CreateTerminalRequest) (acp.CreateTerminalResponse, error) {
	return acp.CreateTerminalResponse{}, fmt.Errorf("terminals not supported yet")
}

func (c *acpClientImpl) KillTerminal(ctx context.Context, params acp.KillTerminalRequest) (acp.KillTerminalResponse, error) {
	return acp.KillTerminalResponse{}, fmt.Errorf("terminals not supported yet")
}

func (c *acpClientImpl) TerminalOutput(ctx context.Context, params acp.TerminalOutputRequest) (acp.TerminalOutputResponse, error) {
	return acp.TerminalOutputResponse{}, fmt.Errorf("terminals not supported yet")
}

func (c *acpClientImpl) ReleaseTerminal(ctx context.Context, params acp.ReleaseTerminalRequest) (acp.ReleaseTerminalResponse, error) {
	return acp.ReleaseTerminalResponse{}, fmt.Errorf("terminals not supported yet")
}

func (c *acpClientImpl) WaitForTerminalExit(ctx context.Context, params acp.WaitForTerminalExitRequest) (acp.WaitForTerminalExitResponse, error) {
	return acp.WaitForTerminalExitResponse{}, fmt.Errorf("terminals not supported yet")
}

// Transport manages a single agent process connection.
type Transport struct {
	cmd  *exec.Cmd
	conn *acp.ClientSideConnection
}

func NewTransport() *Transport {
	return &Transport{}
}

func (t *Transport) Start(ctx context.Context, command string, args []string, workdir string, impl *acpClientImpl) error {
	t.cmd = exec.CommandContext(ctx, command, args...)
	t.cmd.Dir = workdir
	// Inherit stderr so agent crash logs are visible in the daemon output.
	t.cmd.Stderr = os.Stderr
	stdin, err := t.cmd.StdinPipe()
	if err != nil {
		return fmt.Errorf("stdin pipe: %w", err)
	}
	stdout, err := t.cmd.StdoutPipe()
	if err != nil {
		return fmt.Errorf("stdout pipe: %w", err)
	}
	if err := t.cmd.Start(); err != nil {
		return fmt.Errorf("start: %w", err)
	}

	t.conn = acp.NewClientSideConnection(impl, stdin, stdout)
	// Suppress ACP SDK diagnostic logging (e.g. "connection closed") —
	// these are noisy during normal operation and not actionable.
	t.conn.SetLogger(slog.New(slog.NewTextHandler(io.Discard, nil)))
	return nil
}

func (t *Transport) Initialize(ctx context.Context) (acp.InitializeResponse, error) {
	return t.conn.Initialize(ctx, acp.InitializeRequest{
		ClientInfo: &acp.Implementation{
			Name:    "LocalAgentInterface",
			Version: "1.0",
		},
		ClientCapabilities: acp.ClientCapabilities{},
	})
}

func (t *Transport) NewSession(ctx context.Context, cwd string) (string, error) {
	result, err := t.conn.NewSession(ctx, acp.NewSessionRequest{
		Cwd:        cwd,
		McpServers: []acp.McpServer{},
	})
	if err != nil {
		return "", err
	}
	return string(result.SessionId), nil
}

func (t *Transport) Prompt(ctx context.Context, sessionID, content string) error {
	_, err := t.conn.Prompt(ctx, acp.PromptRequest{
		SessionId: acp.SessionId(sessionID),
		Prompt: []acp.ContentBlock{
			acp.TextBlock(content),
		},
	})
	return err
}

func (t *Transport) Cancel(ctx context.Context, sessionID string) error {
	err := t.conn.Cancel(ctx, acp.CancelNotification{
		SessionId: acp.SessionId(sessionID),
	})
	return err
}

func (t *Transport) Close() error {
	if t.cmd != nil && t.cmd.Process != nil {
		_ = t.cmd.Process.Kill()
		_ = t.cmd.Wait()
	}
	return nil
}
