// Package main implements a mock ACP agent for testing.
//
// This agent speaks the ACP protocol over stdio using the acp-go-sdk.
// It streams text responses in chunks, runs real shell commands (ls, pwd,
// echo), and reports tool call results — simulating a real agent like
// Claude Code or Codex CLI without needing an API key.
//
// Usage: mockagent
// (reads ACP from stdin, writes ACP to stdout, logs to stderr)
package main

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"io"
	"log/slog"
	"os"
	"os/exec"
	"runtime"
	"strings"
	"time"

	acp "github.com/coder/acp-go-sdk"
)

// mockAgent implements the acp.Agent interface.
// It streams canned responses, runs real shell commands, and reports
// tool call results to exercise the full ACP pipeline.
type mockAgent struct {
	conn *acp.AgentSideConnection
}

var _ acp.Agent = (*mockAgent)(nil)

func (a *mockAgent) SetAgentConnection(c *acp.AgentSideConnection) { a.conn = c }

func (a *mockAgent) Initialize(_ context.Context, _ acp.InitializeRequest) (acp.InitializeResponse, error) {
	return acp.InitializeResponse{
		ProtocolVersion: acp.ProtocolVersionNumber,
		AgentInfo:       &acp.Implementation{Name: "MockAgent", Version: "1.0.0"},
		AgentCapabilities: acp.AgentCapabilities{
			LoadSession: false,
		},
	}, nil
}

func (a *mockAgent) Authenticate(_ context.Context, _ acp.AuthenticateRequest) (acp.AuthenticateResponse, error) {
	return acp.AuthenticateResponse{}, nil
}

func (a *mockAgent) Logout(_ context.Context, _ acp.LogoutRequest) (acp.LogoutResponse, error) {
	return acp.LogoutResponse{}, nil
}

func (a *mockAgent) NewSession(_ context.Context, _ acp.NewSessionRequest) (acp.NewSessionResponse, error) {
	id := randomSessionID()
	return acp.NewSessionResponse{SessionId: acp.SessionId(id)}, nil
}

// Prompt handles a user prompt by streaming a response with real shell commands.
// It simulates a real agent: thinks, runs `ls`, streams text, runs `pwd`,
// streams more text, then completes.
func (a *mockAgent) Prompt(ctx context.Context, req acp.PromptRequest) (acp.PromptResponse, error) {
	sid := req.SessionId

	// Extract the user's text from the prompt content blocks.
	userText := ""
	for _, block := range req.Prompt {
		if block.Text != nil {
			userText += block.Text.Text
		}
	}

	// 1. Emit a thought
	_ = a.conn.SessionUpdate(ctx, acp.SessionNotification{
		SessionId: sid,
		Update:    acp.UpdateAgentThoughtText("Analyzing the request..."),
	})

	// 2. Start a tool call — list directory contents
	listCmd := "ls"
	if runtime.GOOS == "windows" {
		listCmd = "dir"
	}
	_ = a.conn.SessionUpdate(ctx, acp.SessionNotification{
		SessionId: sid,
		Update: acp.StartToolCall("tool_ls", "List directory",
			acp.WithStartKind(acp.ToolKindExecute),
			acp.WithStartStatus(acp.ToolCallStatusInProgress),
			acp.WithStartRawInput(map[string]any{"command": listCmd}),
		),
	})

	lsOutput := runShellCommand(ctx, listCmd)
	_ = a.conn.SessionUpdate(ctx, acp.SessionNotification{
		SessionId: sid,
		Update: acp.UpdateToolCall("tool_ls",
			acp.WithUpdateStatus(acp.ToolCallStatusCompleted),
			acp.WithUpdateTitle("Run ls — completed"),
			acp.WithUpdateRawOutput(map[string]any{"exitCode": 0, "output": lsOutput}),
		),
	})

	// 3. Stream the first part of the response
	firstChunk := fmt.Sprintf("I received your message: %q\n\nHere's what I found in the current directory:\n%s\n", userText, lsOutput)
	streamText(ctx, a.conn, sid, firstChunk, 20*time.Millisecond)

	// 4. Run `pwd` as another tool call
	pwdCmd := "pwd"
	if runtime.GOOS == "windows" {
		pwdCmd = "cd"
	}
	_ = a.conn.SessionUpdate(ctx, acp.SessionNotification{
		SessionId: sid,
		Update: acp.StartToolCall("tool_pwd", "Print working directory",
			acp.WithStartKind(acp.ToolKindExecute),
			acp.WithStartStatus(acp.ToolCallStatusInProgress),
			acp.WithStartRawInput(map[string]any{"command": pwdCmd}),
		),
	})

	pwdOutput := runShellCommand(ctx, pwdCmd)
	_ = a.conn.SessionUpdate(ctx, acp.SessionNotification{
		SessionId: sid,
		Update: acp.UpdateToolCall("tool_pwd",
			acp.WithUpdateStatus(acp.ToolCallStatusCompleted),
			acp.WithUpdateTitle("Run pwd — completed"),
			acp.WithUpdateRawOutput(map[string]any{"exitCode": 0, "output": pwdOutput}),
		),
	})

	// 5. Stream the final part
	finalChunk := fmt.Sprintf("\nThe current working directory is: %s\n\nAll done!", strings.TrimSpace(pwdOutput))
	streamText(ctx, a.conn, sid, finalChunk, 20*time.Millisecond)

	return acp.PromptResponse{StopReason: acp.StopReasonEndTurn}, nil
}

func (a *mockAgent) Cancel(_ context.Context, _ acp.CancelNotification) error { return nil }

func (a *mockAgent) CloseSession(_ context.Context, _ acp.CloseSessionRequest) (acp.CloseSessionResponse, error) {
	return acp.CloseSessionResponse{}, nil
}

func (a *mockAgent) ListSessions(_ context.Context, _ acp.ListSessionsRequest) (acp.ListSessionsResponse, error) {
	return acp.ListSessionsResponse{}, acp.NewMethodNotFound(acp.AgentMethodSessionList)
}

func (a *mockAgent) ResumeSession(_ context.Context, _ acp.ResumeSessionRequest) (acp.ResumeSessionResponse, error) {
	return acp.ResumeSessionResponse{}, acp.NewMethodNotFound(acp.AgentMethodSessionResume)
}

func (a *mockAgent) SetSessionConfigOption(_ context.Context, _ acp.SetSessionConfigOptionRequest) (acp.SetSessionConfigOptionResponse, error) {
	return acp.SetSessionConfigOptionResponse{}, acp.NewMethodNotFound(acp.AgentMethodSessionSetConfigOption)
}

func (a *mockAgent) SetSessionMode(_ context.Context, _ acp.SetSessionModeRequest) (acp.SetSessionModeResponse, error) {
	return acp.SetSessionModeResponse{}, nil
}

// streamText sends text in small chunks with a delay to simulate streaming.
func streamText(ctx context.Context, conn *acp.AgentSideConnection, sid acp.SessionId, text string, delay time.Duration) {
	words := strings.Fields(text)
	for i, word := range words {
		chunk := word
		if i < len(words)-1 {
			chunk += " "
		}
		_ = conn.SessionUpdate(ctx, acp.SessionNotification{
			SessionId: sid,
			Update:    acp.UpdateAgentMessageText(chunk),
		})
		time.Sleep(delay)
	}
}

// runShellCommand runs a shell command and returns its stdout output.
// On Windows, it uses cmd.exe; on Unix, it uses sh.
func runShellCommand(ctx context.Context, cmd string) string {
	var out []byte
	var err error
	if runtime.GOOS == "windows" {
		out, err = exec.CommandContext(ctx, "cmd", "/c", cmd).Output() //nolint:gosec // cmd is a hardcoded internal mock command (ls/dir/pwd/cd), not user input.
	} else {
		out, err = exec.CommandContext(ctx, "sh", "-c", cmd).Output() //nolint:gosec // cmd is a hardcoded internal mock command (ls/dir/pwd/cd), not user input.
	}
	if err != nil {
		return fmt.Sprintf("error: %v", err)
	}
	return string(out)
}

func randomSessionID() string {
	b := make([]byte, 8)
	_, _ = rand.Read(b)
	return "mock-" + hex.EncodeToString(b)
}

func main() {
	ag := &mockAgent{}
	conn := acp.NewAgentSideConnection(ag, os.Stdout, os.Stdin)
	// Suppress SDK diagnostic logging — stderr is for our own logs.
	conn.SetLogger(slog.New(slog.NewTextHandler(io.Discard, nil)))
	ag.SetAgentConnection(conn)
	<-conn.Done()
}
