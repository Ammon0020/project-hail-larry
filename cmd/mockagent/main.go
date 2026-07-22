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
	"sync"
	"time"

	acp "github.com/coder/acp-go-sdk"
)

// osWindows is the runtime.GOOS value for Windows, used by platform branches.
const osWindows = "windows"

// envNoModeCap, when set to a non-empty value, makes the mock agent NOT
// advertise the `mode`-category `profile` config option. Contract tests use
// this to exercise the client's prompt-injection fallback branch (where the
// client skips `session/set_config_option` and injects profile instructions
// into the prompt instead).
const envNoModeCap = "MOCKAGENT_NO_MODE_CAP"

// profileConfigID is the SessionConfigOption id the Rust client sends via
// `session/set_config_option` to switch the active profile (S-PROF-ACP).
const profileConfigID acp.SessionConfigId = "profile"

// profileMarkerPrefix prefixes the mock's first streamed reply chunk with the
// active profile so contract tests can assert the client sent
// `set_config_option { configId: profile, value: X }` by observing the mock's
// output. Picked over a stderr log line because the existing ACP test harness
// drains stderr without inspecting it, while streamed agent message text is
// surfaced through the conversation pipeline.
const profileMarkerPrefix = "[profile: %s] "

// mockAgent implements the acp.Agent interface.
// It streams canned responses, runs real shell commands, and reports
// tool call results to exercise the full ACP pipeline.
type mockAgent struct {
	conn *acp.AgentSideConnection

	mu       sync.Mutex
	profiles map[string]string // session id -> last received profile value
	modeCap  bool              // whether to advertise the mode/profile config option
}

var _ acp.Agent = (*mockAgent)(nil)

func (a *mockAgent) SetAgentConnection(c *acp.AgentSideConnection) { a.conn = c }

// profileConfigOption builds the `mode`-category `profile` select option the
// client uses to gate the ACP send path. A single placeholder value is
// advertised for UX completeness; SetSessionConfigOption accepts arbitrary
// profile ids without validating against this list.
func (a *mockAgent) profileConfigOption(currentValue string) acp.SessionConfigOption {
	modeCat := acp.SessionConfigOptionCategoryMode
	return acp.SessionConfigOption{
		Select: &acp.SessionConfigOptionSelect{
			Id:           profileConfigID,
			Name:         "Profile",
			Category:     &modeCat,
			CurrentValue: acp.SessionConfigValueId(currentValue),
			Options: acp.SessionConfigSelectOptions{
				Ungrouped: &acp.SessionConfigSelectOptionsUngrouped{
					{Name: "Default", Value: acp.SessionConfigValueId("default")},
				},
			},
			Type: "select",
		},
	}
}

// profileFor returns the last recorded profile value for a session (empty if none).
func (a *mockAgent) profileFor(sessionID string) string {
	a.mu.Lock()
	defer a.mu.Unlock()
	return a.profiles[sessionID]
}

// setProfile records the profile value for a session and returns the new value.
func (a *mockAgent) setProfile(sessionID, value string) string {
	a.mu.Lock()
	defer a.mu.Unlock()
	if a.profiles == nil {
		a.profiles = make(map[string]string)
	}
	a.profiles[sessionID] = value
	return value
}

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
	resp := acp.NewSessionResponse{SessionId: acp.SessionId(id)}
	// Advertise the mode/profile config option so the Rust client's capability
	// gate takes the `session/set_config_option` branch. Suppressed when
	// MOCKAGENT_NO_MODE_CAP is set so the prompt-injection fallback is testable.
	if a.modeCap {
		resp.ConfigOptions = []acp.SessionConfigOption{a.profileConfigOption("")}
	}
	return resp, nil
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
	if runtime.GOOS == osWindows {
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

	// 3. Stream the first part of the response. Prefix with the active profile
	// marker so contract tests can assert the client sent set_config_option.
	firstChunk := fmt.Sprintf("I received your message: %q\n\nHere's what I found in the current directory:\n%s\n", userText, lsOutput)
	if profile := a.profileFor(string(sid)); profile != "" {
		firstChunk = fmt.Sprintf(profileMarkerPrefix, profile) + firstChunk
	}
	streamText(ctx, a.conn, sid, firstChunk, 20*time.Millisecond)

	// 4. Run `pwd` as another tool call
	pwdCmd := "pwd"
	if runtime.GOOS == osWindows {
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

func (a *mockAgent) CloseSession(_ context.Context, req acp.CloseSessionRequest) (acp.CloseSessionResponse, error) {
	a.mu.Lock()
	if a.profiles != nil {
		delete(a.profiles, string(req.SessionId))
	}
	a.mu.Unlock()
	return acp.CloseSessionResponse{}, nil
}

func (a *mockAgent) ListSessions(_ context.Context, _ acp.ListSessionsRequest) (acp.ListSessionsResponse, error) {
	return acp.ListSessionsResponse{}, acp.NewMethodNotFound(acp.AgentMethodSessionList)
}

func (a *mockAgent) ResumeSession(_ context.Context, _ acp.ResumeSessionRequest) (acp.ResumeSessionResponse, error) {
	return acp.ResumeSessionResponse{}, acp.NewMethodNotFound(acp.AgentMethodSessionResume)
}

// SetSessionConfigOption records the requested config option value per
// session. Only the `profile` (mode-category) option is meaningful for the
// profile-over-ACP contract tests; other options are accepted without error.
// The handler succeeds even when the capability was not advertised
// (MOCKAGENT_NO_MODE_CAP), so a misbehaving client still gets a clean
// response — the capability gate is the client's responsibility.
func (a *mockAgent) SetSessionConfigOption(_ context.Context, req acp.SetSessionConfigOptionRequest) (acp.SetSessionConfigOptionResponse, error) {
	sessionID := ""
	value := ""
	configID := acp.SessionConfigId("")
	switch {
	case req.ValueId != nil:
		sessionID = string(req.ValueId.SessionId)
		configID = req.ValueId.ConfigId
		value = string(req.ValueId.Value)
	case req.Boolean != nil:
		sessionID = string(req.Boolean.SessionId)
		configID = req.Boolean.ConfigId
		value = fmt.Sprintf("%v", req.Boolean.Value)
	}

	if configID == profileConfigID {
		recorded := a.setProfile(sessionID, value)
		// Echo the recorded value to stderr as a secondary, greppable signal
		// for harnesses that capture child stderr.
		slog.Info("set_config_option profile recorded",
			"sessionId", sessionID, "profile", recorded)
	}

	return acp.SetSessionConfigOptionResponse{
		ConfigOptions: []acp.SessionConfigOption{a.profileConfigOption(value)},
	}, nil
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
	if runtime.GOOS == osWindows {
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
	ag := &mockAgent{
		profiles: make(map[string]string),
		// Default to advertising the mode/profile capability; suppress only when
		// the no-cap env var is explicitly set, so contract tests can exercise
		// the client's prompt-injection fallback branch.
		modeCap: os.Getenv(envNoModeCap) == "",
	}
	conn := acp.NewAgentSideConnection(ag, os.Stdout, os.Stdin)
	// Suppress SDK diagnostic logging — stderr is for our own logs.
	conn.SetLogger(slog.New(slog.NewTextHandler(io.Discard, nil)))
	ag.SetAgentConnection(conn)
	<-conn.Done()
}
