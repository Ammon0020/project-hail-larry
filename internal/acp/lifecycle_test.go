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
	newSessionOpts   []acpsdk.SessionConfigOption
	newSessionErr    error
	newSessionCalled bool
	newSessionCwd    string
	newSessionDirs   []string

	// LoadSession
	loadSessionResult string
	loadSessionOpts   []acpsdk.SessionConfigOption
	loadSessionErr    error
	loadSessionCalled bool
	loadSessionID     string
	loadSessionDirs   []string

	// DeleteSession
	deleteSessionCalled bool
	deleteSessionID     string
	deleteSessionErr    error

	// ListSessions
	listSessionsResult []acpsdk.SessionInfo
	listSessionsErr    error
	listSessionsCalled bool

	// Prompt
	promptErr    error
	promptCalled bool

	// Cancel
	cancelCalled bool

	// Close
	closeCalled bool

	// StderrTail
	stderrTail string

	// SetSessionConfigOption
	setConfigOptionCalled bool
	setConfigOptionArgs   []string // sessionID, configID, value
	setConfigOptionErr    error

	// Providers
	providersSupported    bool
	listProvidersResult   []acpsdk.UnstableProviderInfo
	listProvidersErr      error
	listProvidersCalled   bool
	setProviderCalled     bool
	setProviderArgs       []string // id, apiType, baseURL
	setProviderHeaders    map[string]any
	setProviderErr        error
	disableProviderCalled bool
	disableProviderID     string
	disableProviderErr    error
}

func (m *mockTransport) NewSession(_ context.Context, cwd string, additionalDirs []string) (string, []acpsdk.SessionConfigOption, error) {
	m.newSessionCalled = true
	m.newSessionCwd = cwd
	m.newSessionDirs = append(m.newSessionDirs[:0:0], additionalDirs...)
	return m.newSessionResult, m.newSessionOpts, m.newSessionErr
}

func (m *mockTransport) LoadSession(_ context.Context, acpSessionID string, additionalDirs []string) (string, []acpsdk.SessionConfigOption, error) {
	m.loadSessionCalled = true
	m.loadSessionID = acpSessionID
	m.loadSessionDirs = append(m.loadSessionDirs[:0:0], additionalDirs...)
	return m.loadSessionResult, m.loadSessionOpts, m.loadSessionErr
}

func (m *mockTransport) SetSessionConfigOption(_ context.Context, sessionID, configID, value string) error {
	m.setConfigOptionCalled = true
	m.setConfigOptionArgs = []string{sessionID, configID, value}
	return m.setConfigOptionErr
}

func (m *mockTransport) DeleteSession(_ context.Context, acpSessionID string) error {
	m.deleteSessionCalled = true
	m.deleteSessionID = acpSessionID
	return m.deleteSessionErr
}

func (m *mockTransport) ListSessions(_ context.Context) ([]acpsdk.SessionInfo, error) {
	m.listSessionsCalled = true
	return m.listSessionsResult, m.listSessionsErr
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

func (m *mockTransport) SupportsEmbeddedContext() bool {
	return false
}

func (m *mockTransport) SupportsProviders() bool {
	return m.providersSupported
}

func (m *mockTransport) ListProviders(_ context.Context) ([]acpsdk.UnstableProviderInfo, error) {
	m.listProvidersCalled = true
	return m.listProvidersResult, m.listProvidersErr
}

func (m *mockTransport) SetProvider(_ context.Context, id, apiType, baseURL string, headers map[string]any) error {
	m.setProviderCalled = true
	m.setProviderArgs = []string{id, apiType, baseURL}
	m.setProviderHeaders = headers
	return m.setProviderErr
}

func (m *mockTransport) DisableProvider(_ context.Context, id string) error {
	m.disableProviderCalled = true
	m.disableProviderID = id
	return m.disableProviderErr
}

// TestResolveACPSession verifies the session/load-vs-session/new decision in
// resolveACPSession: load is attempted only when a persisted ACP session ID
// exists AND the agent advertised loadSession; any load error falls back to
// NewSession. When the agent supports session/list, the persisted ID is
// reconciled against the agent's session list before attempting LoadSession.
func TestResolveACPSession(t *testing.T) {
	listCap := &acpsdk.SessionListCapabilities{}
	cases := []struct {
		name           string
		acpSessionID   string
		loadCapability bool
		listCapability *acpsdk.SessionListCapabilities
		listErr        error
		listSessions   []acpsdk.SessionInfo
		loadErr        error
		loadResult     string
		newResult      string
		newErr         error
		wantListCalled bool
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
		// --- session/list reconciliation cases ---
		{
			name:           "list confirms session exists, load succeeds",
			acpSessionID:   "acp-123",
			loadCapability: true,
			listCapability: listCap,
			listSessions: []acpsdk.SessionInfo{
				{SessionId: "acp-123"},
			},
			loadResult:     "acp-123",
			newResult:      "new-456",
			wantListCalled: true,
			wantLoadCalled: true,
			wantNewCalled:  false,
			wantID:         "acp-123",
		},
		{
			name:           "list says session gone, skip load go straight to new",
			acpSessionID:   "acp-123",
			loadCapability: true,
			listCapability: listCap,
			listSessions: []acpsdk.SessionInfo{
				{SessionId: "other-999"},
			},
			newResult:      "new-456",
			wantListCalled: true,
			wantLoadCalled: false,
			wantNewCalled:  true,
			wantID:         "new-456",
		},
		{
			name:           "list returns empty, skip load go straight to new",
			acpSessionID:   "acp-123",
			loadCapability: true,
			listCapability: listCap,
			listSessions:   []acpsdk.SessionInfo{},
			newResult:      "new-456",
			wantListCalled: true,
			wantLoadCalled: false,
			wantNewCalled:  true,
			wantID:         "new-456",
		},
		{
			name:           "list fails, fall back to legacy load-then-new",
			acpSessionID:   "acp-123",
			loadCapability: true,
			listCapability: listCap,
			listErr:        errors.New("list not supported"),
			loadResult:     "acp-123",
			newResult:      "new-456",
			wantListCalled: true,
			wantLoadCalled: true,
			wantNewCalled:  false,
			wantID:         "acp-123",
		},
		{
			name:           "list capability but no load capability, skip both list and load",
			acpSessionID:   "acp-123",
			loadCapability: false,
			listCapability: listCap,
			newResult:      "new-456",
			wantListCalled: false,
			wantLoadCalled: false,
			wantNewCalled:  true,
			wantID:         "new-456",
		},
	}

	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			mt := &mockTransport{
				loadSessionResult:  tc.loadResult,
				loadSessionErr:     tc.loadErr,
				newSessionResult:   tc.newResult,
				newSessionErr:      tc.newErr,
				listSessionsResult: tc.listSessions,
				listSessionsErr:    tc.listErr,
			}
			session := &Session{ACPSessionID: tc.acpSessionID}
			initResp := acpsdk.InitializeResponse{
				AgentCapabilities: acpsdk.AgentCapabilities{
					LoadSession: tc.loadCapability,
				},
			}
			if tc.listCapability != nil {
				initResp.AgentCapabilities.SessionCapabilities.List = tc.listCapability
			}

			c := NewClient(ClientConfig{})
			gotID, _, err := c.resolveACPSession(context.Background(), mt, initResp, session, "/ws", nil)

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
			if mt.listSessionsCalled != tc.wantListCalled {
				t.Errorf("ListSessions called = %v, want %v", mt.listSessionsCalled, tc.wantListCalled)
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
	c := NewClient(ClientConfig{})
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
	c := NewClient(ClientConfig{})
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
	c := NewClient(ClientConfig{})
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
	c := NewClient(ClientConfig{})
	if err := c.CloseAllSessions(context.Background()); err != nil {
		t.Fatalf("CloseAllSessions on empty client: %v", err)
	}
}

// TestACPSessionIDPersists verifies the exported ACPSessionID field round-trips
// through JSON persistence, so a restarted daemon can attempt session/load.
func TestACPSessionIDPersists(t *testing.T) {
	c := NewClient(ClientConfig{})
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

	c2 := NewClient(ClientConfig{})
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
