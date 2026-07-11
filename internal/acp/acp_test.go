package acp

import (
	"context"
	"encoding/json"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"

	acpsdk "github.com/coder/acp-go-sdk"
)

// mockCallbacks captures events for testing.
type mockCallbacks struct {
	mu     sync.Mutex
	events []interfaces.Event
}

func (m *mockCallbacks) OnEvent(event interfaces.Event) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.events = append(m.events, event)
}

// TestRegisterAndListAgents verifies agent registration and listing.
func TestRegisterAndListAgents(t *testing.T) {
	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "claude-code",
		Name:    "Claude Code",
		Command: "claude",
		Models: []AgentModel{
			{ID: "claude-sonnet-4", Name: "Claude Sonnet 4"},
			{ID: "claude-opus-4", Name: "Claude Opus 4"},
		},
	})

	agents, err := client.ListAgents(ctx)
	if err != nil {
		t.Fatalf("list agents: %v", err)
	}
	if len(agents) != 1 {
		t.Fatalf("expected 1 agent, got %d", len(agents))
	}
	if agents[0].Name != "Claude Code" {
		t.Errorf("expected name 'Claude Code', got %s", agents[0].Name)
	}
	if len(agents[0].Models) != 2 {
		t.Errorf("expected 2 models, got %d", len(agents[0].Models))
	}
}

// TestCreateSession verifies session creation with a valid agent and model.
func TestCreateSession(t *testing.T) {
	t.Skip("Requires a mock ACP agent for testing real stdio transport")
	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "codex",
		Name:    "Codex CLI",
		Command: "go",
		Args:    []string{"version"},
		Models:  []AgentModel{{ID: "gpt-4", Name: "GPT-4"}},
	})

	session, err := client.CreateSession(ctx, "codex", "gpt-4", ".")
	if err != nil {
		t.Fatalf("create session: %v", err)
	}
	if session.ID == "" {
		t.Error("expected non-empty session ID")
	}
	if session.Status != "created" {
		t.Errorf("expected status 'created', got %s", session.Status)
	}
}

// TestCreateSessionInvalidAgent verifies that creating a session with an unknown agent fails.
// This fails at the agent lookup before any transport is spawned.
func TestCreateSessionInvalidAgent(t *testing.T) {
	client := NewClient(nil, nil)
	ctx := context.Background()

	_, err := client.CreateSession(ctx, "nonexistent", "model", ".")
	if err == nil {
		t.Error("expected error for unknown agent")
	}
}

// TestCreateSessionInvalidModel verifies that using an unoffered model fails.
// This fails at model validation before any transport is spawned.
func TestCreateSessionInvalidModel(t *testing.T) {
	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "agent-1",
		Name:    "Agent 1",
		Command: "go",
		Args:    []string{"version"},
		Models:  []AgentModel{{ID: "model-a", Name: "Model A"}},
	})

	_, err := client.CreateSession(ctx, "agent-1", "model-b", ".")
	if err == nil {
		t.Error("expected error for invalid model")
	}
}

// TestSendPrompt verifies that sending a prompt emits an event.
func TestSendPrompt(t *testing.T) {
	t.Skip("Requires a mock ACP agent for testing real stdio transport")
	client := NewClient(nil, nil)
	ctx := context.Background()
	cb := &mockCallbacks{}
	client.SetCallbacks(cb)

	client.RegisterAgent(AgentInfo{
		ID:      "agent-1",
		Name:    "Agent 1",
		Command: "go",
		Args:    []string{"version"},
		Models:  []AgentModel{{ID: "model-a", Name: "Model A"}},
	})

	session, _ := client.CreateSession(ctx, "agent-1", "model-a", ".")

	err := client.SendPrompt(ctx, session.ID, "Hello, agent!", nil)
	if err != nil {
		t.Fatalf("send prompt: %v", err)
	}

	// Verify an event was emitted.
	if len(cb.events) == 0 {
		t.Fatal("expected at least one event")
	}

	// Find the prompt event (the last one should be the prompt).
	lastEvent := cb.events[len(cb.events)-1]
	if lastEvent.Type != interfaces.EventPromptSubmitted {
		t.Errorf("expected event type PromptSubmitted, got %s", lastEvent.Type)
	}
	if lastEvent.Content != "Hello, agent!" {
		t.Errorf("expected content 'Hello, agent!', got %s", lastEvent.Content)
	}
	if lastEvent.Role != "user" {
		t.Errorf("expected role 'user', got %s", lastEvent.Role)
	}
}

// TestSendPromptInvalidSession verifies that sending a prompt to a nonexistent session fails.
// This fails at session lookup before any transport is touched.
func TestSendPromptInvalidSession(t *testing.T) {
	client := NewClient(nil, nil)
	ctx := context.Background()

	err := client.SendPrompt(ctx, "nonexistent", "hello", nil)
	if err == nil {
		t.Error("expected error for nonexistent session")
	}
}

// TestCancelSession verifies that cancelling a session updates its status.
func TestCancelSession(t *testing.T) {
	t.Skip("Requires a mock ACP agent for testing real stdio transport")
	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "agent-1",
		Name:    "Agent 1",
		Command: "go",
		Args:    []string{"version"},
		Models:  []AgentModel{{ID: "model-a", Name: "Model A"}},
	})

	session, _ := client.CreateSession(ctx, "agent-1", "model-a", ".")

	err := client.CancelSession(ctx, session.ID)
	if err != nil {
		t.Fatalf("cancel session: %v", err)
	}

	s, _ := client.GetSession(session.ID)
	if s.Status != "interrupted" {
		t.Errorf("expected status 'interrupted', got %s", s.Status)
	}
}

// TestCloseSession verifies that closing a session removes it.
func TestCloseSession(t *testing.T) {
	t.Skip("Requires a mock ACP agent for testing real stdio transport")
	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "agent-1",
		Name:    "Agent 1",
		Command: "go",
		Args:    []string{"version"},
		Models:  []AgentModel{{ID: "model-a", Name: "Model A"}},
	})

	session, _ := client.CreateSession(ctx, "agent-1", "model-a", ".")

	err := client.CloseSession(ctx, session.ID)
	if err != nil {
		t.Fatalf("close session: %v", err)
	}

	_, err = client.GetSession(session.ID)
	if err == nil {
		t.Error("expected error for closed session")
	}
}

// TestListSessions verifies that all active sessions are listed.
func TestListSessions(t *testing.T) {
	t.Skip("Requires a mock ACP agent for testing real stdio transport")
	client := NewClient(nil, nil)
	ctx := context.Background()

	client.RegisterAgent(AgentInfo{
		ID:      "agent-1",
		Name:    "Agent 1",
		Command: "go",
		Args:    []string{"version"},
		Models:  []AgentModel{{ID: "model-a", Name: "Model A"}},
	})

	client.CreateSession(ctx, "agent-1", "model-a", ".")
	client.CreateSession(ctx, "agent-1", "model-a", ".")

	sessions := client.ListSessions()
	if len(sessions) != 2 {
		t.Fatalf("expected 2 sessions, got %d", len(sessions))
	}
}

// TestNewSessionRequestMcpServersIsEmptyList verifies that the ACP NewSessionRequest
// serializes mcpServers as an empty list rather than null. A null value causes
// Pydantic validation errors in strict ACP agents (e.g., devstral-small).
func TestNewSessionRequestMcpServersIsEmptyList(t *testing.T) {
	req := acpsdk.NewSessionRequest{
		Cwd:        "/tmp",
		McpServers: []acpsdk.McpServer{},
	}
	data, err := json.Marshal(req)
	if err != nil {
		t.Fatalf("marshal NewSessionRequest: %v", err)
	}
	if strings.Contains(string(data), `"mcpServers":null`) {
		t.Errorf("NewSessionRequest serialized mcpServers as null: %s", data)
	}
	if !strings.Contains(string(data), `"mcpServers":[]`) {
		t.Errorf("NewSessionRequest did not serialize mcpServers as []: %s", data)
	}
}

// TestResolveCwd verifies that resolveCwd rejects agent-supplied working
// directories that escape the workspace root and falls back to the workspace
// path. This closes the terminal-cwd escape where an agent could request a
// terminal with Cwd set to ~/.ssh, /etc, or C:\Windows.
func TestResolveCwd(t *testing.T) {
	root := "/home/user/project"
	if runtime.GOOS == "windows" {
		root = `C:\Users\user\project`
	}

	tests := []struct {
		name      string
		candidate string
		want      string // expected resolved cwd (empty == want root)
	}{
		{name: "empty falls back to root", candidate: "", want: root},
		{name: "workspace root itself", candidate: root, want: root},
		{name: "subdirectory inside workspace", candidate: filepath.Join(root, "src"), want: filepath.Join(root, "src")},
		{name: "nested subdirectory inside workspace", candidate: filepath.Join(root, "src", "cmd"), want: filepath.Join(root, "src", "cmd")},
		{name: "parent directory escapes", candidate: filepath.Dir(root), want: root},
		{name: "sibling directory escapes", candidate: filepath.Join(filepath.Dir(root), "other"), want: root},
		{name: "traversal via dotdot escapes", candidate: filepath.Join(root, "..", ".."), want: root},
	}

	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			got := resolveCwd(root, tc.candidate)
			want := tc.want
			if want == "" {
				want = root
			}
			if got != want {
				t.Errorf("resolveCwd(%q, %q) = %q, want %q", root, tc.candidate, got, want)
			}
		})
	}
}

// TestEnvToSlice verifies that agent-supplied ACP EnvVariable entries are
// converted into the "KEY=VALUE" slice format expected by exec.Cmd.Env, and
// that entries with empty names are dropped.
func TestEnvToSlice(t *testing.T) {
	vars := []acpsdk.EnvVariable{
		{Name: "PATH", Value: "/usr/local/bin:/usr/bin"},
		{Name: "API_KEY", Value: "secret"},
		{Name: "", Value: "dropped"},
		{Name: "EMPTY", Value: ""},
	}
	got := envToSlice(vars)
	want := []string{
		"PATH=/usr/local/bin:/usr/bin",
		"API_KEY=secret",
		"EMPTY=",
	}
	if len(got) != len(want) {
		t.Fatalf("got %v, want %v", got, want)
	}
	for i := range want {
		if got[i] != want[i] {
			t.Errorf("index %d: got %q, want %q", i, got[i], want[i])
		}
	}
}

// TestCreateTerminalEnvPassesThrough verifies that env variables supplied in a
// CreateTerminalRequest are visible to the spawned subprocess (i.e. they are
// not silently dropped in favor of the daemon environment).
func TestCreateTerminalEnvPassesThrough(t *testing.T) {
	dir := t.TempDir()
	c := &acpClientImpl{
		workspacePath: dir,
		sessionID:     "test-session",
		terminals:     make(map[string]*terminalEntry),
	}

	// Use a marker var name unlikely to collide with the daemon environment.
	const marker = "ACP_TEST_ENV_PASS_THROUGH"
	t.Setenv(marker, "") // ensure clean slate in this test process

	var command string
	var args []string
	if runtime.GOOS == "windows" {
		command = "cmd"
		args = []string{"/C", "echo %" + marker + "%"}
	} else {
		command = "sh"
		args = []string{"-c", "printf %s \"$" + marker + "\""}
	}

	resp, err := c.CreateTerminal(context.Background(), acpsdk.CreateTerminalRequest{
		Command: command,
		Args:    args,
		Env:     []acpsdk.EnvVariable{{Name: marker, Value: "honored"}},
	})
	if err != nil {
		t.Fatalf("CreateTerminal: %v", err)
	}

	_, err = c.WaitForTerminalExit(context.Background(), acpsdk.WaitForTerminalExitRequest{TerminalId: resp.TerminalId})
	if err != nil {
		t.Fatalf("WaitForTerminalExit: %v", err)
	}

	out, _, _ := c.getTerminal(resp.TerminalId).snapshot()
	if got := strings.TrimSpace(out); got != "honored" {
		t.Errorf("expected subprocess to see %s=honored, got output %q", marker, got)
	}
}
