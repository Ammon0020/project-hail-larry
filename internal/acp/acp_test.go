package acp

import (
	"context"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
)

// mockCallbacks captures events for testing.
type mockCallbacks struct {
	events []interfaces.Event
}

func (m *mockCallbacks) OnEvent(event interfaces.Event) {
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

	err := client.SendPrompt(ctx, session.ID, "Hello, agent!")
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

	err := client.SendPrompt(ctx, "nonexistent", "hello")
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
