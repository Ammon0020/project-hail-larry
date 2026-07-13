package server

import (
	"context"
	"encoding/json"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/interfaces"
	acpsdk "github.com/coder/acp-go-sdk"
)

// stubProviderTransport is a minimal acp.TransportLike implementation for the
// provider handlers. Only the provider methods are wired; the rest return zero
// values so the stub satisfies the interface without dragging in real session
// behavior.
type stubProviderTransport struct {
	supported     bool
	listResult    []acpsdk.UnstableProviderInfo
	listErr       error
	setErr        error
	disableErr    error
	setCalled     bool
	setID         string
	setAPIType    string
	setBaseURL    string
	disableCalled bool
	disableID     string
}

func (s *stubProviderTransport) NewSession(_ context.Context, _ string, _ []string) (string, []acpsdk.SessionConfigOption, error) {
	return "", nil, nil
}
func (s *stubProviderTransport) LoadSession(_ context.Context, _ string, _ []string) (string, []acpsdk.SessionConfigOption, error) {
	return "", nil, nil
}
func (s *stubProviderTransport) ListSessions(_ context.Context) ([]acpsdk.SessionInfo, error) {
	return nil, nil
}
func (s *stubProviderTransport) DeleteSession(_ context.Context, _ string) error { return nil }
func (s *stubProviderTransport) Prompt(_ context.Context, _ string, _ string, _ []acp.ContextResource, _ []interfaces.Attachment) (acpsdk.StopReason, error) {
	return "", nil
}
func (s *stubProviderTransport) Cancel(_ context.Context, _ string) error { return nil }
func (s *stubProviderTransport) Close() error                             { return nil }
func (s *stubProviderTransport) StderrTail() string                       { return "" }
func (s *stubProviderTransport) SetSessionConfigOption(_ context.Context, _, _, _ string) error {
	return nil
}
func (s *stubProviderTransport) SupportsEmbeddedContext() bool { return false }
func (s *stubProviderTransport) SupportsProviders() bool       { return s.supported }
func (s *stubProviderTransport) ListProviders(_ context.Context) ([]acpsdk.UnstableProviderInfo, error) {
	return s.listResult, s.listErr
}
func (s *stubProviderTransport) SetProvider(_ context.Context, id, apiType, baseURL string, _ map[string]any) error {
	s.setCalled = true
	s.setID = id
	s.setAPIType = apiType
	s.setBaseURL = baseURL
	return s.setErr
}
func (s *stubProviderTransport) DisableProvider(_ context.Context, id string) error {
	s.disableCalled = true
	s.disableID = id
	return s.disableErr
}

// newProvidersServer builds a server with an acp.Client seeded with one session
// ("sess-prov") whose transport is the given stub. Auth is skipped (no
// PairingMgr) so the tests can hit the handlers directly.
func newProvidersServer(t *testing.T, tr *stubProviderTransport) *Server {
	t.Helper()
	client := acp.NewClient(acp.ClientConfig{})
	now := time.Now().UTC().Truncate(time.Second)
	seed := []acp.Session{{
		ID:        "sess-prov",
		Name:      "Providers test",
		AgentID:   "agent-1",
		ModelID:   "model-a",
		Workspace: "ws-1",
		Status:    "idle",
		CreatedAt: now,
		UpdatedAt: now,
	}}
	data, err := json.MarshalIndent(seed, "", "  ")
	if err != nil {
		t.Fatalf("marshal seed: %v", err)
	}
	storePath := filepath.Join(t.TempDir(), "conversations.json")
	if err := os.WriteFile(storePath, data, 0o600); err != nil {
		t.Fatalf("write store: %v", err)
	}
	client.SetStorePath(storePath)
	if err := client.LoadConversations(); err != nil {
		t.Fatalf("load conversations: %v", err)
	}
	if err := client.SetTransportForTest("sess-prov", tr); err != nil {
		t.Fatalf("inject transport: %v", err)
	}
	return New(&Deps{ACPClient: client})
}

func TestHandleListProviders_Success(t *testing.T) {
	tr := &stubProviderTransport{
		supported: true,
		listResult: []acpsdk.UnstableProviderInfo{
			{Id: "main", Required: true, Supported: []acpsdk.UnstableLlmProtocol{acpsdk.UnstableLlmProtocolAnthropic}},
			{Id: "openai", Supported: []acpsdk.UnstableLlmProtocol{acpsdk.UnstableLlmProtocolOpenai}},
		},
	}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodGet, "/api/sessions/sess-prov/providers", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}
	var got []interfaces.ProviderInfo
	if err := json.Unmarshal(rec.Body.Bytes(), &got); err != nil {
		t.Fatalf("unmarshal: %v", err)
	}
	if len(got) != 2 {
		t.Fatalf("expected 2 providers, got %d", len(got))
	}
	if got[0].ID != "main" || !got[0].Required {
		t.Errorf("provider 0: %+v", got[0])
	}
	// Empty array, not null, when non-empty result.
	if strings.Contains(rec.Body.String(), "null") {
		t.Errorf("body should not contain null: %s", rec.Body.String())
	}
}

func TestHandleListProviders_Unsupported(t *testing.T) {
	tr := &stubProviderTransport{supported: false}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodGet, "/api/sessions/sess-prov/providers", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusNotImplemented {
		t.Fatalf("expected 501, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

func TestHandleListProviders_SessionNotFound(t *testing.T) {
	tr := &stubProviderTransport{supported: true}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodGet, "/api/sessions/nope/providers", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusNotFound {
		t.Fatalf("expected 404 for missing session, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

func TestHandleSetProvider_Success(t *testing.T) {
	tr := &stubProviderTransport{supported: true}
	srv := newProvidersServer(t, tr)

	body := `{"apiType":"openai","baseUrl":"https://api.openai.com","headers":{"Authorization":"Bearer sk-x"}}`
	req := httptest.NewRequest(http.MethodPut, "/api/sessions/sess-prov/providers/openai", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}
	if !tr.setCalled || tr.setID != "openai" || tr.setAPIType != "openai" || tr.setBaseURL != "https://api.openai.com" {
		t.Errorf("set not forwarded correctly: %+v", tr)
	}
}

func TestHandleSetProvider_MissingAPIType(t *testing.T) {
	tr := &stubProviderTransport{supported: true}
	srv := newProvidersServer(t, tr)

	body := `{"baseUrl":"https://x"}`
	req := httptest.NewRequest(http.MethodPut, "/api/sessions/sess-prov/providers/openai", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for missing apiType, got %d", rec.Code)
	}
	if tr.setCalled {
		t.Errorf("SetProvider should not be called on validation failure")
	}
}

func TestHandleSetProvider_MissingBaseURL(t *testing.T) {
	tr := &stubProviderTransport{supported: true}
	srv := newProvidersServer(t, tr)

	body := `{"apiType":"openai"}`
	req := httptest.NewRequest(http.MethodPut, "/api/sessions/sess-prov/providers/openai", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for missing baseUrl, got %d", rec.Code)
	}
}

func TestHandleSetProvider_Unsupported(t *testing.T) {
	tr := &stubProviderTransport{supported: false}
	srv := newProvidersServer(t, tr)

	body := `{"apiType":"openai","baseUrl":"https://x"}`
	req := httptest.NewRequest(http.MethodPut, "/api/sessions/sess-prov/providers/openai", strings.NewReader(body))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusNotImplemented {
		t.Fatalf("expected 501, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

func TestHandleSetProvider_MalformedBody(t *testing.T) {
	tr := &stubProviderTransport{supported: true}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodPut, "/api/sessions/sess-prov/providers/openai", strings.NewReader("{bad"))
	req.Header.Set("Content-Type", "application/json")
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400, got %d", rec.Code)
	}
}

func TestHandleDisableProvider_Success(t *testing.T) {
	tr := &stubProviderTransport{
		supported: true,
		listResult: []acpsdk.UnstableProviderInfo{
			{Id: "openai", Required: false, Supported: []acpsdk.UnstableLlmProtocol{acpsdk.UnstableLlmProtocolOpenai}},
		},
	}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodDelete, "/api/sessions/sess-prov/providers/openai", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusOK {
		t.Fatalf("expected 200, got %d (body: %s)", rec.Code, rec.Body.String())
	}
	if !tr.disableCalled || tr.disableID != "openai" {
		t.Errorf("disable not forwarded: %+v", tr)
	}
}

func TestHandleDisableProvider_RequiredRefused(t *testing.T) {
	tr := &stubProviderTransport{
		supported: true,
		listResult: []acpsdk.UnstableProviderInfo{
			{Id: "main", Required: true, Supported: []acpsdk.UnstableLlmProtocol{acpsdk.UnstableLlmProtocolAnthropic}},
		},
	}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodDelete, "/api/sessions/sess-prov/providers/main", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusBadRequest {
		t.Fatalf("expected 400 for required provider, got %d (body: %s)", rec.Code, rec.Body.String())
	}
	if tr.disableCalled {
		t.Errorf("DisableProvider should not be called for a required provider")
	}
}

func TestHandleDisableProvider_Unsupported(t *testing.T) {
	tr := &stubProviderTransport{supported: false}
	srv := newProvidersServer(t, tr)

	req := httptest.NewRequest(http.MethodDelete, "/api/sessions/sess-prov/providers/openai", nil)
	rec := httptest.NewRecorder()
	srv.Handler().ServeHTTP(rec, req)

	if rec.Code != http.StatusNotImplemented {
		t.Fatalf("expected 501, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}
