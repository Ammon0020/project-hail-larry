package acp

import (
	"context"
	"path/filepath"
	"testing"
	"time"
)

// TestConversationPersistAndReload verifies that conversation metadata survives
// a daemon restart: a session persisted by one client is loaded by another.
func TestConversationPersistAndReload(t *testing.T) {
	storePath := filepath.Join(t.TempDir(), "conversations.json")

	c1 := NewClient(nil, nil)
	c1.SetStorePath(storePath)

	// Insert a conversation directly (avoids spawning a real agent process) and
	// persist it the way the lifecycle methods do.
	c1.mu.Lock()
	c1.sessions["sess-test"] = &Session{
		ID:        "sess-test",
		Name:      defaultConversationName,
		AgentID:   "codex",
		ModelID:   "gpt-4o",
		Status:    "created",
		CreatedAt: time.Now().UTC(),
		UpdatedAt: time.Now().UTC(),
	}
	c1.persistLocked()
	c1.mu.Unlock()

	// Rename via the public API and confirm it persists.
	if err := c1.RenameSession("sess-test", "My refactor"); err != nil {
		t.Fatalf("rename: %v", err)
	}

	// A fresh client loads the persisted conversation.
	c2 := NewClient(nil, nil)
	c2.SetStorePath(storePath)
	if err := c2.LoadConversations(); err != nil {
		t.Fatalf("load: %v", err)
	}

	sessions := c2.ListSessions()
	if len(sessions) != 1 {
		t.Fatalf("expected 1 loaded conversation, got %d", len(sessions))
	}
	got := sessions[0]
	if got.Name != "My refactor" {
		t.Errorf("expected name 'My refactor', got %q", got.Name)
	}
	if got.AgentID != "codex" || got.ModelID != "gpt-4o" {
		t.Errorf("expected codex/gpt-4o, got %s/%s", got.AgentID, got.ModelID)
	}
	if got.Status != "idle" {
		t.Errorf("expected loaded status 'idle', got %q", got.Status)
	}
}

// TestRebindSessionValidatesAgentModel verifies rebind rejects unknown
// agent/model and updates the record on success, preserving the id.
func TestRebindSessionValidatesAgentModel(t *testing.T) {
	c := NewClient(nil, nil)
	c.RegisterAgent(AgentInfo{
		ID:     "vibe",
		Name:   "Mistral Vibe",
		Models: []AgentModel{{ID: "mistral-large", Name: "Mistral Large"}},
	})

	c.mu.Lock()
	c.sessions["sess-1"] = &Session{ID: "sess-1", Name: "chat", AgentID: "codex", ModelID: "gpt-4o", Status: "idle"}
	c.mu.Unlock()

	ctx := context.Background()

	if _, err := c.RebindSession(ctx, "sess-1", "vibe", "no-such-model"); err == nil {
		t.Error("expected error for invalid model")
	}

	info, err := c.RebindSession(ctx, "sess-1", "vibe", "mistral-large")
	if err != nil {
		t.Fatalf("rebind: %v", err)
	}
	if info.ID != "sess-1" {
		t.Errorf("expected id preserved, got %q", info.ID)
	}

	s, _ := c.GetSession("sess-1")
	if s.AgentID != "vibe" || s.ModelID != "mistral-large" {
		t.Errorf("expected vibe/mistral-large, got %s/%s", s.AgentID, s.ModelID)
	}
}

// TestTitleFromPrompt verifies conversation auto-titling from the first prompt.
func TestTitleFromPrompt(t *testing.T) {
	cases := map[string]string{
		"Refactor the auth flow":  "Refactor the auth flow",
		"line one\nline two":      "line one",
		"":                        defaultConversationName,
		"   trimmed   ":           "trimmed",
	}
	for in, want := range cases {
		if got := titleFromPrompt(in); got != want {
			t.Errorf("titleFromPrompt(%q) = %q, want %q", in, got, want)
		}
	}
}
