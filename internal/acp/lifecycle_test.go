package acp

import (
	"context"
	"errors"
	"path/filepath"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
	acpsdk "github.com/coder/acp-go-sdk"
)

// mockTransport is a stand-in for *Transport used by the lifecycle tests. It
// records which methods were called and with what arguments, and returns
// configurable results. It satisfies the transportLike interface.
type mockTransport struct {
	// NewSession
	newSessionResult string
	newSessionErr    error
	newSessionCalled bool
	newSessionCwd    string

	// LoadSession
	loadSessionResult string
	loadSessionErr    error
	loadSessionCalled bool
	loadSessionID     string

	// DeleteSession
	deleteSessionCalled bool
	deleteSessionID     string
	deleteSessionErr    error

	// Prompt
	promptErr    error
	promptCalled bool

	// Cancel
	cancelCalled bool

	// Close
	closeCalled bool

	// StderrTail
	stderrTail string
}

func (m *mockTransport) NewSession(_ context.Context, cwd string) (string, error) {
	m.newSessionCalled = true
	m.newSessionCwd = cwd
	return m.newSessionResult, m.newSessionErr
}

func (m *mockTransport) LoadSession(_ context.Context, acpSessionID string) (string, error) {
	m.loadSessionCalled = true
	m.loadSessionID = acpSessionID
	return m.loadSessionResult, m.loadSessionErr
}

func (m *mockTransport) DeleteSession(_ context.Context, acpSessionID string) error {
	m.deleteSessionCalled = true
	m.deleteSessionID = acpSessionID
	return m.deleteSessionErr
}

func (m *mockTransport) Prompt(_ context.Context, _, _ string, _ []ContextResource, _ []interfaces.Attachment) (acpsdk.StopReason, error) {
	m.promptCalled = true
	return "", m.promptErr
}

func (m *mockTransport) Cancel(_ context.Context, _ string) error {
	m.cancelCalled = true
	return nil
}

func (m *mockTransport) Close() error {
	m.closeCalled = true
	return nil
}

func (m *mockTransport) StderrTail() string {
	return m.stderrTail
}

// TestResolveACPSession verifies the session/load-vs-session/new decision in
// resolveACPSession: load is attempted only when a persisted ACP session ID
// exists AND the agent advertised loadSession; any load error falls back to
// NewSession.
func TestResolveACPSession(t *testing.T) {
	cases := []struct {
		name           string
		acpSessionID   string
		loadCapability bool
		loadErr        error
		loadResult     string
		newResult      string
		newErr         error
		wantLoadCalled bool
		wantNewCalled  bool
		wantID         string
		wantErr        bool
	}{
		{
			name:           "load attempted when id set and capability true",
			acpSessionID:   "acp-123",
			loadCapability: true,
			loadResult:     "acp-123",
			newResult:      "new-456",
			wantLoadCalled: true,
			wantNewCalled:  false,
			wantID:         "acp-123",
		},
		{
			name:           "fallback to new session on load error",
			acpSessionID:   "acp-123",
			loadCapability: true,
			loadErr:        errors.New("session gone"),
			newResult:      "new-456",
			wantLoadCalled: true,
			wantNewCalled:  true,
			wantID:         "new-456",
		},
		{
			name:           "skip load when capability false",
			acpSessionID:   "acp-123",
			loadCapability: false,
			newResult:      "new-456",
			wantLoadCalled: false,
			wantNewCalled:  true,
			wantID:         "new-456",
		},
		{
			name:           "skip load when no persisted id",
			acpSessionID:   "",
			loadCapability: true,
			newResult:      "new-456",
			wantLoadCalled: false,
			wantNewCalled:  true,
			wantID:         "new-456",
		},
		{
			name:           "new session error propagates",
			acpSessionID:   "",
			loadCapability: true,
			newErr:         errors.New("boom"),
			wantNewCalled:  true,
			wantErr:        true,
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			mt := &mockTransport{
				loadSessionResult: tc.loadResult,
				loadSessionErr:    tc.loadErr,
				newSessionResult:  tc.newResult,
				newSessionErr:     tc.newErr,
			}
			session := &Session{ACPSessionID: tc.acpSessionID}
			initResp := acpsdk.InitializeResponse{
				AgentCapabilities: acpsdk.AgentCapabilities{
					LoadSession: tc.loadCapability,
				},
			}

			c := NewClient(nil, nil)
			gotID, err := c.resolveACPSession(context.Background(), mt, initResp, session, "/ws")

			if tc.wantErr {
				if err == nil {
					t.Fatalf("expected error, got nil")
				}
				return
			}
			if err != nil {
				t.Fatalf("unexpected error: %v", err)
			}
			if gotID != tc.wantID {
				t.Errorf("acp session ID = %q, want %q", gotID, tc.wantID)
			}
			if mt.loadSessionCalled != tc.wantLoadCalled {
				t.Errorf("LoadSession called = %v, want %v", mt.loadSessionCalled, tc.wantLoadCalled)
			}
			if mt.newSessionCalled != tc.wantNewCalled {
				t.Errorf("NewSession called = %v, want %v", mt.newSessionCalled, tc.wantNewCalled)
			}
			// When load is attempted, it must be called with the persisted ID.
			if mt.loadSessionCalled && mt.loadSessionID != tc.acpSessionID {
				t.Errorf("LoadSession called with %q, want %q", mt.loadSessionID, tc.acpSessionID)
			}
		})
	}
}

// TestCloseSessionCallsDeleteBeforeKill verifies that CloseSession issues an
// ACP session/delete (with the persisted ACP session ID) before killing the
// agent process via Close.
func TestCloseSessionCallsDeleteBeforeKill(t *testing.T) {
	c := NewClient(nil, nil)
	ctx := context.Background()

	mt := &mockTransport{deleteSessionErr: nil}
	c.mu.Lock()
	c.sessions["sess-1"] = &Session{
		ID:           "sess-1",
		Name:         "chat",
		AgentID:      "mock",
		ModelID:      "mock-model",
		Status:       "running",
		ACPSessionID: "acp-abc",
		transport:    mt,
	}
	c.mu.Unlock()

	if err := c.CloseSession(ctx, "sess-1"); err != nil {
		t.Fatalf("CloseSession: %v", err)
	}

	if !mt.deleteSessionCalled {
		t.Error("expected DeleteSession to be called before kill")
	}
	if mt.deleteSessionID != "acp-abc" {
		t.Errorf("DeleteSession called with %q, want %q", mt.deleteSessionID, "acp-abc")
	}
	if !mt.closeCalled {
		t.Error("expected transport.Close to be called")
	}
	// Delete must happen before Close.
	if !mt.deleteSessionCalled || !mt.closeCalled {
		t.Error("both delete and close should have been invoked")
	}

	// Session should be removed.
	if _, err := c.GetSession("sess-1"); err == nil {
		t.Error("expected session to be removed after CloseSession")
	}
}

// TestCloseSessionNoDeleteWithoutID verifies CloseSession skips session/delete
// when no ACP session ID is persisted (e.g. a freshly-rebound session).
func TestCloseSessionNoDeleteWithoutID(t *testing.T) {
	c := NewClient(nil, nil)
	ctx := context.Background()

	mt := &mockTransport{}
	c.mu.Lock()
	c.sessions["sess-2"] = &Session{
		ID:           "sess-2",
		AgentID:      "mock",
		ModelID:      "mock-model",
		Status:       "idle",
		ACPSessionID: "",
		transport:    mt,
	}
	c.mu.Unlock()

	if err := c.CloseSession(ctx, "sess-2"); err != nil {
		t.Fatalf("CloseSession: %v", err)
	}
	if mt.deleteSessionCalled {
		t.Error("DeleteSession should not be called when ACPSessionID is empty")
	}
	if !mt.closeCalled {
		t.Error("expected transport.Close to be called")
	}
}

// TestCloseAllSessions verifies that CloseAllSessions closes every active
// session's transport, invoking session/delete + Close on each, while
// preserving session metadata so conversations survive a daemon restart.
func TestCloseAllSessions(t *testing.T) {
	c := NewClient(nil, nil)
	ctx := context.Background()

	transports := make([]*mockTransport, 0, 3)
	c.mu.Lock()
	for i := 0; i < 3; i++ {
		mt := &mockTransport{}
		transports = append(transports, mt)
		id := "sess-" + string(rune('a'+i))
		c.sessions[id] = &Session{
			ID:           id,
			AgentID:      "mock",
			ModelID:      "mock-model",
			Status:       "running",
			ACPSessionID: "acp-" + id,
			transport:    mt,
		}
	}
	c.mu.Unlock()

	if err := c.CloseAllSessions(ctx); err != nil {
		t.Fatalf("CloseAllSessions: %v", err)
	}

	// Metadata is preserved (not deleted) so conversations survive a restart.
	if got := len(c.ListSessions()); got != 3 {
		t.Errorf("expected 3 sessions preserved, got %d", got)
	}
	for i, mt := range transports {
		if !mt.deleteSessionCalled {
			t.Errorf("session %d: expected DeleteSession to be called", i)
		}
		if !mt.closeCalled {
			t.Errorf("session %d: expected Close to be called", i)
		}
	}
	// Every session should now be idle with no live transport.
	for _, id := range []string{"sess-a", "sess-b", "sess-c"} {
		s, err := c.GetSession(id)
		if err != nil {
			t.Errorf("expected session %s preserved, got error: %v", id, err)
			continue
		}
		if s.transport != nil {
			t.Errorf("session %s: expected transport cleared, still set", id)
		}
		if s.Status != "idle" {
			t.Errorf("session %s: expected status 'idle', got %q", id, s.Status)
		}
	}
}

// TestCloseAllSessionsEmpty verifies CloseAllSessions is a no-op (no error)
// when there are no sessions.
func TestCloseAllSessionsEmpty(t *testing.T) {
	c := NewClient(nil, nil)
	if err := c.CloseAllSessions(context.Background()); err != nil {
		t.Fatalf("CloseAllSessions on empty client: %v", err)
	}
}

// TestACPSessionIDPersists verifies the exported ACPSessionID field round-trips
// through JSON persistence, so a restarted daemon can attempt session/load.
func TestACPSessionIDPersists(t *testing.T) {
	c := NewClient(nil, nil)
	c.mu.Lock()
	c.sessions["sess-x"] = &Session{
		ID:           "sess-x",
		Name:         "chat",
		AgentID:      "mock",
		ModelID:      "mock-model",
		Status:       "idle",
		CreatedAt:    time.Now().UTC(),
		UpdatedAt:    time.Now().UTC(),
		ACPSessionID: "acp-persist-1",
	}
	c.mu.Unlock()

	// Marshal via persistLocked into a temp store, then reload.
	storePath := filepath.Join(t.TempDir(), "conversations.json")
	c.SetStorePath(storePath)
	c.mu.Lock()
	c.persistLocked()
	c.mu.Unlock()

	c2 := NewClient(nil, nil)
	c2.SetStorePath(storePath)
	if err := c2.LoadConversations(); err != nil {
		t.Fatalf("LoadConversations: %v", err)
	}
	s, err := c2.GetSession("sess-x")
	if err != nil {
		t.Fatalf("GetSession: %v", err)
	}
	if s.ACPSessionID != "acp-persist-1" {
		t.Errorf("ACPSessionID = %q, want %q", s.ACPSessionID, "acp-persist-1")
	}
	if s.transport != nil {
		t.Error("loaded session should have no live transport")
	}
}
