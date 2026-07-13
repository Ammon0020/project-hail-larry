package acp

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// mockEventStore is an in-memory interfaces.EventStore for conversation export
// tests. It stores events per session and returns them in insertion order via
// Query. Append assigns monotonic IDs; QueryAll returns all events across
// sessions in ID order.
type mockEventStore struct {
	events []interfaces.Event
	nextID int64
}

func (s *mockEventStore) Append(_ context.Context, e interfaces.Event) (interfaces.Event, error) {
	s.nextID++
	e.ID = s.nextID
	if e.Timestamp.IsZero() {
		e.Timestamp = time.Now().UTC()
	}
	s.events = append(s.events, e)
	return e, nil
}

func (s *mockEventStore) Query(_ context.Context, sessionID string, afterID int64, _ int) ([]interfaces.Event, error) {
	var out []interfaces.Event
	for _, e := range s.events {
		if e.SessionID != sessionID || e.ID <= afterID {
			continue
		}
		out = append(out, e)
	}
	return out, nil
}

func (s *mockEventStore) QueryAll(_ context.Context, afterID int64, _ int) ([]interfaces.Event, error) {
	var out []interfaces.Event
	for _, e := range s.events {
		if e.ID <= afterID {
			continue
		}
		out = append(out, e)
	}
	return out, nil
}

// TestConversationPersistAndReload verifies that conversation metadata survives
// a daemon restart: a session persisted by one client is loaded by another.
func TestConversationPersistAndReload(t *testing.T) {
	storePath := filepath.Join(t.TempDir(), "conversations.json")

	c1 := NewClient(ClientConfig{})
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
	c2 := NewClient(ClientConfig{})
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
	c := NewClient(ClientConfig{})
	c.RegisterAgent(AgentInfo{
		ID:     "vibe",
		Name:   "Mistral Vibe",
		Models: []AgentModel{{ID: "mistral-large", Name: "Mistral Large"}},
	})

	c.mu.Lock()
	c.sessions["sess-1"] = &Session{ID: "sess-1", Name: "chat", AgentID: "codex", ModelID: "gpt-4o", Status: "idle"}
	c.mu.Unlock()

	ctx := context.Background()

	if _, err := c.RebindSession(ctx, "sess-1", "vibe", "no-such-model", 0); err == nil {
		t.Error("expected error for invalid model")
	}

	info, err := c.RebindSession(ctx, "sess-1", "vibe", "mistral-large", 0)
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

	c1 := NewClient(ClientConfig{})
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
	if unmarshalErr := json.Unmarshal(data, &records); unmarshalErr != nil {
		t.Fatalf("unmarshal store: %v", unmarshalErr)
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
	c2 := NewClient(ClientConfig{})
	c2.SetStorePath(storePath)
	if loadErr := c2.LoadConversations(); loadErr != nil {
		t.Fatalf("LoadConversations: %v", loadErr)
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

// --- Conversation export tests ---------------------------------------------

// TestExportConversation_RendersTranscript verifies that ExportConversation
// formats a simple user→assistant→user→assistant exchange as markdown with the
// expected **User:** and **Assistant:** labels, and skips internal events.
func TestExportConversation_RendersTranscript(t *testing.T) {
	store := &mockEventStore{}
	ctx := context.Background()
	sid := "sess-export"

	// User prompt.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventPromptSubmitted, SessionID: sid, Role: "user", Content: "Can you fix the bug in auth.go?",
	})
	// ResponseStarted (internal "thinking" indicator) — should be skipped.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventResponseStarted, SessionID: sid, Content: "Agent is thinking…",
	})
	// Assistant stream chunks.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventStreamUpdate, SessionID: sid, Role: "agent", Content: "I'll look at auth.go...",
	})
	// Tool call.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventToolStarted, SessionID: sid, Tool: "read_file",
	})
	// ToolCompleted — should be skipped (already summarized by ToolStarted).
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventToolCompleted, SessionID: sid, Tool: "read_file", Summary: "read 200 lines",
	})
	// More assistant text.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventStreamUpdate, SessionID: sid, Role: "agent", Content: "The fix is to add a mutex...",
	})
	// Terminal empty StreamUpdate — should be skipped.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventStreamUpdate, SessionID: sid, Role: "agent", Content: "", Streaming: false,
	})
	// Second user prompt.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventPromptSubmitted, SessionID: sid, Role: "user", Content: "Great, now add tests.",
	})
	// Internal event — should be skipped.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventConnectionRestarted, SessionID: sid, Content: "restarted",
	})

	out, err := ExportConversation(ctx, store, sid, 0)
	if err != nil {
		t.Fatalf("ExportConversation: %v", err)
	}

	// The transcript should contain the user/assistant labels and tool summary,
	// and NOT contain the "thinking" indicator or the restart event.
	if !strings.Contains(out, "**User:** Can you fix the bug in auth.go?") {
		t.Errorf("expected first user message, got %q", out)
	}
	if !strings.Contains(out, "**Assistant:** I'll look at auth.go...") {
		t.Errorf("expected first assistant message, got %q", out)
	}
	if !strings.Contains(out, "[Tool: read_file]") {
		t.Errorf("expected tool summary, got %q", out)
	}
	if !strings.Contains(out, "**Assistant:** The fix is to add a mutex...") {
		t.Errorf("expected second assistant message, got %q", out)
	}
	if !strings.Contains(out, "**User:** Great, now add tests.") {
		t.Errorf("expected second user message, got %q", out)
	}
	if strings.Contains(out, "Agent is thinking") {
		t.Errorf("ResponseStarted should be skipped, got %q", out)
	}
	if strings.Contains(out, "restarted") {
		t.Errorf("ConnectionRestarted should be skipped, got %q", out)
	}
	if strings.Contains(out, "read 200 lines") {
		t.Errorf("ToolCompleted summary should be skipped, got %q", out)
	}
}

// TestExportConversation_Truncation verifies that a transcript exceeding
// maxBytes is truncated and ends with the truncation note.
func TestExportConversation_Truncation(t *testing.T) {
	store := &mockEventStore{}
	ctx := context.Background()
	sid := "sess-trunc"

	// Build a large conversation by appending many user prompts.
	for i := 0; i < 50; i++ {
		_, _ = store.Append(ctx, interfaces.Event{
			Type: interfaces.EventPromptSubmitted, SessionID: sid, Role: "user",
			Content: strings.Repeat("X", 100),
		})
	}

	maxBytes := 500
	out, err := ExportConversation(ctx, store, sid, maxBytes)
	if err != nil {
		t.Fatalf("ExportConversation: %v", err)
	}
	if len(out) > maxBytes {
		t.Errorf("expected output ≤ %d bytes, got %d", maxBytes, len(out))
	}
	if !strings.Contains(out, "[... conversation truncated,") {
		t.Errorf("expected truncation note, got tail %q", out[len(out)-80:])
	}
	if !strings.HasSuffix(out, "bytes total ...]") {
		t.Errorf("expected truncation note at end, got %q", out[len(out)-80:])
	}
}

// TestExportConversation_NilStoreReturnsEmpty verifies that a nil event store
// yields an empty string without error.
func TestExportConversation_NilStoreReturnsEmpty(t *testing.T) {
	out, err := ExportConversation(context.Background(), nil, "s1", 100)
	if err != nil {
		t.Fatalf("unexpected error: %v", err)
	}
	if out != "" {
		t.Errorf("expected empty output for nil store, got %q", out)
	}
}

// TestExportConversation_EmptySession verifies that a session with no events
// produces an empty transcript.
func TestExportConversation_EmptySession(t *testing.T) {
	store := &mockEventStore{}
	out, err := ExportConversation(context.Background(), store, "no-events", 100)
	if err != nil {
		t.Fatalf("ExportConversation: %v", err)
	}
	if out != "" {
		t.Errorf("expected empty transcript for session with no events, got %q", out)
	}
}

// --- ConversationTransferMiddleware tests ----------------------------------

// TestConversationTransferMiddleware_InjectsOnFirstPrompt verifies that a
// queued transfer is injected when PromptCount == 0 and the queue is cleared
// afterward.
func TestConversationTransferMiddleware_InjectsOnFirstPrompt(t *testing.T) {
	mw := NewConversationTransferMiddleware(nil)
	mw.SetTransfer("s1", "Prior conversation transcript here.", "Claude Code")

	pc := &PromptContext{SessionID: "s1", PromptCount: 0}
	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionInject {
		t.Fatalf("expected ActionInject, got %v", action)
	}
	if !strings.Contains(msg, "## Previous Conversation (transferred from Claude Code)") {
		t.Errorf("expected transfer header with agent name, got %q", msg)
	}
	if !strings.Contains(msg, "Prior conversation transcript here.") {
		t.Errorf("expected transcript body, got %q", msg)
	}

	// Queue should be cleared — a second call (even with PromptCount 0) injects
	// nothing.
	action2, msg2 := mw.BeforePrompt(context.Background(), &PromptContext{SessionID: "s1", PromptCount: 0})
	if action2 != ActionContinue {
		t.Errorf("expected ActionContinue after clear, got %v", action2)
	}
	if msg2 != "" {
		t.Errorf("expected empty message after clear, got %q", msg2)
	}
}

// TestConversationTransferMiddleware_SkipsOnLaterPrompt verifies that a queued
// transfer is NOT injected when PromptCount != 0 (the first-prompt window has
// passed) and is cleared so it does not leak.
func TestConversationTransferMiddleware_SkipsOnLaterPrompt(t *testing.T) {
	mw := NewConversationTransferMiddleware(nil)
	mw.SetTransfer("s1", "transcript", "Codex")

	pc := &PromptContext{SessionID: "s1", PromptCount: 1}
	action, msg := mw.BeforePrompt(context.Background(), pc)
	if action != ActionContinue {
		t.Errorf("expected ActionContinue on non-first prompt, got %v", action)
	}
	if msg != "" {
		t.Errorf("expected no injection on non-first prompt, got %q", msg)
	}

	// Queue is cleared even though we did not inject.
	action2, _ := mw.BeforePrompt(context.Background(), &PromptContext{SessionID: "s1", PromptCount: 0})
	if action2 != ActionContinue {
		t.Errorf("expected queue cleared after non-first prompt, got %v", action2)
	}
}

// TestConversationTransferMiddleware_EmptyMarkdownSkips verifies that a queued
// transfer with blank markdown is not injected.
func TestConversationTransferMiddleware_EmptyMarkdownSkips(t *testing.T) {
	mw := NewConversationTransferMiddleware(nil)
	mw.SetTransfer("s1", "   \n  ", "Agent")

	action, msg := mw.BeforePrompt(context.Background(), &PromptContext{SessionID: "s1", PromptCount: 0})
	if action != ActionContinue {
		t.Errorf("expected ActionContinue for blank markdown, got %v", action)
	}
	if msg != "" {
		t.Errorf("expected empty message for blank markdown, got %q", msg)
	}
}

// TestConversationTransferMiddleware_NoTransferQueued verifies that with no
// queued transfer the middleware injects nothing.
func TestConversationTransferMiddleware_NoTransferQueued(t *testing.T) {
	mw := NewConversationTransferMiddleware(nil)
	action, msg := mw.BeforePrompt(context.Background(), &PromptContext{SessionID: "s1", PromptCount: 0})
	if action != ActionContinue {
		t.Errorf("expected ActionContinue with no transfer, got %v", action)
	}
	if msg != "" {
		t.Errorf("expected empty message with no transfer, got %q", msg)
	}
}

// --- RebindSession conversation transfer tests -----------------------------

// TestRebindSession_QueuesConversationTransfer verifies that RebindSession
// exports the prior conversation and queues it on the transfer middleware so
// the new agent's first prompt receives it.
func TestRebindSession_QueuesConversationTransfer(t *testing.T) {
	store := &mockEventStore{}
	ctx := context.Background()
	sid := "sess-rebind"

	// Seed the event store with a prior conversation.
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventPromptSubmitted, SessionID: sid, Role: "user", Content: "Fix the bug.",
	})
	_, _ = store.Append(ctx, interfaces.Event{
		Type: interfaces.EventStreamUpdate, SessionID: sid, Role: "agent", Content: "Done.",
	})

	c := NewClient(ClientConfig{})
	c.RegisterAgent(AgentInfo{
		ID:     "old-agent",
		Name:   "Old Agent",
		Models: []AgentModel{{ID: "old-model", Name: "Old Model"}},
	})
	c.RegisterAgent(AgentInfo{
		ID:     "new-agent",
		Name:   "New Agent",
		Models: []AgentModel{{ID: "new-model", Name: "New Model"}},
	})

	transfer := NewConversationTransferMiddleware(nil)
	c.SetEventStore(store)
	c.SetConversationTransfer(transfer)
	cb := &mockCallbacks{}
	c.SetCallbacks(cb)

	// Create the session on the old agent.
	c.mu.Lock()
	c.sessions[sid] = &Session{ID: sid, Name: "chat", AgentID: "old-agent", ModelID: "old-model", Status: "idle"}
	c.mu.Unlock()

	info, err := c.RebindSession(ctx, sid, "new-agent", "new-model", 0)
	if err != nil {
		t.Fatalf("RebindSession: %v", err)
	}
	if info.AgentID != "new-agent" {
		t.Fatalf("expected new agent, got %s", info.AgentID)
	}

	// The transfer middleware should now have a queued transfer for this
	// session. Run BeforePrompt with PromptCount 0 and confirm the transcript
	// is injected with the old agent's name in the header.
	action, msg := transfer.BeforePrompt(context.Background(), &PromptContext{SessionID: sid, PromptCount: 0})
	if action != ActionInject {
		t.Fatalf("expected ActionInject on first prompt after rebind, got %v", action)
	}
	if !strings.Contains(msg, "## Previous Conversation (transferred from Old Agent)") {
		t.Errorf("expected header with old agent name, got %q", msg)
	}
	if !strings.Contains(msg, "**User:** Fix the bug.") {
		t.Errorf("expected exported user message in transcript, got %q", msg)
	}
	if !strings.Contains(msg, "**Assistant:** Done.") {
		t.Errorf("expected exported assistant message in transcript, got %q", msg)
	}

	// The ConnectionRestarted event content should mention the export.
	foundRestart := false
	for _, e := range cb.events {
		if e.Type == interfaces.EventConnectionRestarted {
			foundRestart = true
			if !strings.Contains(e.Content, "exported") {
				t.Errorf("expected restart event to mention export, got %q", e.Content)
			}
		}
	}
	if !foundRestart {
		t.Error("expected ConnectionRestarted event to be emitted")
	}
}

// TestRebindSession_NoTransferWithoutStore verifies that when no event store is
// set, RebindSession still succeeds but does not queue a transfer (the new
// agent starts fresh).
func TestRebindSession_NoTransferWithoutStore(t *testing.T) {
	c := NewClient(ClientConfig{})
	c.RegisterAgent(AgentInfo{
		ID: "a1", Name: "A1", Models: []AgentModel{{ID: "m1", Name: "M1"}},
	})
	c.RegisterAgent(AgentInfo{
		ID: "a2", Name: "A2", Models: []AgentModel{{ID: "m2", Name: "M2"}},
	})
	transfer := NewConversationTransferMiddleware(nil)
	c.SetConversationTransfer(transfer)

	c.mu.Lock()
	c.sessions["s1"] = &Session{ID: "s1", AgentID: "a1", ModelID: "m1", Status: "idle"}
	c.mu.Unlock()

	if _, err := c.RebindSession(context.Background(), "s1", "a2", "m2", 0); err != nil {
		t.Fatalf("RebindSession: %v", err)
	}
	// No transfer queued — BeforePrompt injects nothing.
	action, msg := transfer.BeforePrompt(context.Background(), &PromptContext{SessionID: "s1", PromptCount: 0})
	if action != ActionContinue || msg != "" {
		t.Errorf("expected no injection without event store, got %v / %q", action, msg)
	}
}
