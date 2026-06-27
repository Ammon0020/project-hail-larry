package acp

import (
	"context"
	"encoding/json"
	"os"
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
		"Refactor the auth flow": "Refactor the auth flow",
		"line one\nline two":     "line one",
		"":                       defaultConversationName,
		"   trimmed   ":          "trimmed",
	}
	for in, want := range cases {
		if got := titleFromPrompt(in); got != want {
			t.Errorf("titleFromPrompt(%q) = %q, want %q", in, got, want)
		}
	}
}

// TestCloseAllSessionsPreservesMetadata verifies that CloseAllSessions (used on
// daemon shutdown) closes transports and clears permission policies but does
// NOT delete session metadata from the map or wipe conversations.json. A fresh
// client loading the persisted file should see the session again. This is the
// regression guard for the "conversations lost on daemon restart" bug.
func TestCloseAllSessionsPreservesMetadata(t *testing.T) {
	storePath := filepath.Join(t.TempDir(), "conversations.json")

	c1 := NewClient(nil, nil)
	c1.SetStorePath(storePath)

	mt := &mockTransport{}
	c1.mu.Lock()
	c1.sessions["sess-survive"] = &Session{
		ID:           "sess-survive",
		Name:         "My chat",
		AgentID:      "codex",
		ModelID:      "gpt-4o",
		Workspace:    "ws-1",
		Status:       "running",
		CreatedAt:    time.Now().UTC(),
		UpdatedAt:    time.Now().UTC(),
		ACPSessionID: "acp-123",
		transport:    mt,
	}
	c1.mu.Unlock()

	if err := c1.CloseAllSessions(context.Background()); err != nil {
		t.Fatalf("CloseAllSessions: %v", err)
	}

	// 1. Session still in the in-memory map (metadata preserved).
	c1.mu.Lock()
	sess, ok := c1.sessions["sess-survive"]
	c1.mu.Unlock()
	if !ok {
		t.Fatal("expected session to remain in c.sessions after CloseAllSessions")
	}
	// 2. Transport cleared and status idle.
	if sess.transport != nil {
		t.Error("expected transport to be nil after CloseAllSessions")
	}
	if sess.Status != "idle" {
		t.Errorf("expected status 'idle', got %q", sess.Status)
	}
	// 3. Mock transport had its DeleteSession + Close called.
	if !mt.deleteSessionCalled {
		t.Error("expected DeleteSession to be called on transport")
	}
	if !mt.closeCalled {
		t.Error("expected Close to be called on transport")
	}

	// 4. conversations.json on disk still contains the session.
	data, err := os.ReadFile(storePath)
	if err != nil {
		t.Fatalf("read store: %v", err)
	}
	var records []Session
	if err := json.Unmarshal(data, &records); err != nil {
		t.Fatalf("unmarshal store: %v", err)
	}
	found := false
	for _, r := range records {
		if r.ID == "sess-survive" {
			found = true
			if r.Name != "My chat" || r.AgentID != "codex" || r.ModelID != "gpt-4o" {
				t.Errorf("persisted session fields mismatch: %+v", r)
			}
		}
	}
	if !found {
		t.Error("expected session to be present in conversations.json on disk")
	}

	// 5. A fresh client loads the persisted session.
	c2 := NewClient(nil, nil)
	c2.SetStorePath(storePath)
	if err := c2.LoadConversations(); err != nil {
		t.Fatalf("LoadConversations: %v", err)
	}
	loaded, err := c2.GetSession("sess-survive")
	if err != nil {
		t.Fatalf("GetSession after reload: %v", err)
	}
	if loaded.Name != "My chat" {
		t.Errorf("expected loaded name 'My chat', got %q", loaded.Name)
	}
	if loaded.transport != nil {
		t.Error("loaded session should have no live transport")
	}
	if loaded.Status != "idle" {
		t.Errorf("loaded status = %q, want 'idle'", loaded.Status)
	}
}
