package acp

import (
	"context"
	"errors"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
	acpsdk "github.com/coder/acp-go-sdk"
)

// seedSessionForProviders builds a Client with one session whose transport is
// the given mock, so the provider-management Client methods can be exercised
// without spawning a real agent. The session is inserted directly into the
// client's map (no CreateSession / startTransportLocked).
func seedSessionForProviders(t *testing.T, mt *mockTransport) *Client {
	t.Helper()
	c := NewClient(nil, nil)
	c.sessions["sess-prov"] = &Session{
		ID:        "sess-prov",
		AgentID:   "agent-1",
		transport: mt,
	}
	return c
}

func TestClient_ListProviders_Unsupported(t *testing.T) {
	mt := &mockTransport{providersSupported: false}
	c := seedSessionForProviders(t, mt)

	_, err := c.ListProviders(context.Background(), "sess-prov")
	if !errors.Is(err, ErrProvidersUnsupported) {
		t.Fatalf("expected ErrProvidersUnsupported, got %v", err)
	}
	if mt.listProvidersCalled {
		t.Errorf("ListProviders should not be called when unsupported")
	}
}

func TestClient_ListProviders_ForwardsAndConverts(t *testing.T) {
	mt := &mockTransport{
		providersSupported: true,
		listProvidersResult: []acpsdk.UnstableProviderInfo{
			{
				Id:       "main",
				Required: true,
				Supported: []acpsdk.UnstableLlmProtocol{
					acpsdk.UnstableLlmProtocolAnthropic,
					acpsdk.UnstableLlmProtocolOpenai,
				},
				Current: &acpsdk.UnstableProviderCurrentConfig{
					ApiType: acpsdk.UnstableLlmProtocolAnthropic,
					BaseUrl: "https://api.anthropic.com",
				},
			},
			{
				Id:        "openai",
				Required:  false,
				Supported: []acpsdk.UnstableLlmProtocol{acpsdk.UnstableLlmProtocolOpenai},
				// Current nil → disabled
			},
		},
	}
	c := seedSessionForProviders(t, mt)

	got, err := c.ListProviders(context.Background(), "sess-prov")
	if err != nil {
		t.Fatalf("ListProviders: %v", err)
	}
	if !mt.listProvidersCalled {
		t.Errorf("expected transport.ListProviders to be called")
	}
	if len(got) != 2 {
		t.Fatalf("expected 2 providers, got %d", len(got))
	}
	if got[0].ID != "main" || !got[0].Required {
		t.Errorf("provider 0: %+v", got[0])
	}
	if got[0].Current == nil || got[0].Current.APIType != "anthropic" || got[0].Current.BaseURL != "https://api.anthropic.com" {
		t.Errorf("provider 0 current: %+v", got[0].Current)
	}
	if len(got[0].Supported) != 2 || got[0].Supported[0] != "anthropic" || got[0].Supported[1] != "openai" {
		t.Errorf("provider 0 supported: %+v", got[0].Supported)
	}
	if got[1].Current != nil {
		t.Errorf("provider 1 should be disabled (nil current), got %+v", got[1].Current)
	}
}

func TestClient_ListProviders_SessionNotFound(t *testing.T) {
	c := NewClient(nil, nil)
	_, err := c.ListProviders(context.Background(), "nope")
	if err == nil {
		t.Fatalf("expected error for missing session")
	}
}

func TestClient_ListProviders_TransportError(t *testing.T) {
	mt := &mockTransport{
		providersSupported: true,
		listProvidersErr:   errors.New("boom"),
	}
	c := seedSessionForProviders(t, mt)

	_, err := c.ListProviders(context.Background(), "sess-prov")
	if err == nil || !errors.Is(err, mt.listProvidersErr) {
		t.Fatalf("expected wrapped transport error, got %v", err)
	}
}

func TestClient_SetProvider_Unsupported(t *testing.T) {
	mt := &mockTransport{providersSupported: false}
	c := seedSessionForProviders(t, mt)

	err := c.SetProvider(context.Background(), "sess-prov", "main", "anthropic", "https://x", nil)
	if !errors.Is(err, ErrProvidersUnsupported) {
		t.Fatalf("expected ErrProvidersUnsupported, got %v", err)
	}
	if mt.setProviderCalled {
		t.Errorf("SetProvider should not be called when unsupported")
	}
}

func TestClient_SetProvider_ForwardsAndConvertsHeaders(t *testing.T) {
	mt := &mockTransport{providersSupported: true}
	c := seedSessionForProviders(t, mt)

	err := c.SetProvider(context.Background(), "sess-prov", "main", "openai", "https://api.openai.com",
		map[string]string{"Authorization": "Bearer sk-x"})
	if err != nil {
		t.Fatalf("SetProvider: %v", err)
	}
	if !mt.setProviderCalled {
		t.Errorf("expected transport.SetProvider to be called")
	}
	if mt.setProviderArgs[0] != "main" || mt.setProviderArgs[1] != "openai" || mt.setProviderArgs[2] != "https://api.openai.com" {
		t.Errorf("forwarded args: %+v", mt.setProviderArgs)
	}
	if mt.setProviderHeaders["Authorization"] != "Bearer sk-x" {
		t.Errorf("headers not converted: %+v", mt.setProviderHeaders)
	}
}

func TestClient_SetProvider_NilHeaders(t *testing.T) {
	mt := &mockTransport{providersSupported: true}
	c := seedSessionForProviders(t, mt)

	if err := c.SetProvider(context.Background(), "sess-prov", "main", "openai", "https://x", nil); err != nil {
		t.Fatalf("SetProvider: %v", err)
	}
	if mt.setProviderHeaders != nil {
		t.Errorf("nil headers should stay nil, got %+v", mt.setProviderHeaders)
	}
}

func TestClient_SetProvider_SessionNotFound(t *testing.T) {
	c := NewClient(nil, nil)
	err := c.SetProvider(context.Background(), "nope", "main", "openai", "https://x", nil)
	if err == nil {
		t.Fatalf("expected error for missing session")
	}
}

func TestClient_DisableProvider_Unsupported(t *testing.T) {
	mt := &mockTransport{providersSupported: false}
	c := seedSessionForProviders(t, mt)

	err := c.DisableProvider(context.Background(), "sess-prov", "main")
	if !errors.Is(err, ErrProvidersUnsupported) {
		t.Fatalf("expected ErrProvidersUnsupported, got %v", err)
	}
	if mt.disableProviderCalled {
		t.Errorf("DisableProvider should not be called when unsupported")
	}
}

func TestClient_DisableProvider_Forwards(t *testing.T) {
	mt := &mockTransport{providersSupported: true}
	c := seedSessionForProviders(t, mt)

	if err := c.DisableProvider(context.Background(), "sess-prov", "openai"); err != nil {
		t.Fatalf("DisableProvider: %v", err)
	}
	if !mt.disableProviderCalled || mt.disableProviderID != "openai" {
		t.Errorf("disable not forwarded: called=%v id=%q", mt.disableProviderCalled, mt.disableProviderID)
	}
}

func TestClient_DisableProvider_SessionNotFound(t *testing.T) {
	c := NewClient(nil, nil)
	err := c.DisableProvider(context.Background(), "nope", "main")
	if err == nil {
		t.Fatalf("expected error for missing session")
	}
}

func TestClient_DisableProvider_TransportError(t *testing.T) {
	mt := &mockTransport{
		providersSupported: true,
		disableProviderErr: errors.New("nope"),
	}
	c := seedSessionForProviders(t, mt)

	err := c.DisableProvider(context.Background(), "sess-prov", "main")
	if err == nil || !errors.Is(err, mt.disableProviderErr) {
		t.Fatalf("expected wrapped error, got %v", err)
	}
}

// TestToInterfacesProviders covers the SDK → interface projection, including
// the empty-slice (not nil) result so the REST handler serializes [] not null.
func TestToInterfacesProviders(t *testing.T) {
	t.Run("empty", func(t *testing.T) {
		got := toInterfacesProviders(nil)
		if got == nil {
			t.Fatalf("expected non-nil empty slice, got nil")
		}
		if len(got) != 0 {
			t.Errorf("expected 0, got %d", len(got))
		}
	})
	t.Run("with current and supported", func(t *testing.T) {
		in := []acpsdk.UnstableProviderInfo{
			{
				Id:        "p1",
				Required:  true,
				Supported: []acpsdk.UnstableLlmProtocol{acpsdk.UnstableLlmProtocolAzure},
				Current: &acpsdk.UnstableProviderCurrentConfig{
					ApiType: acpsdk.UnstableLlmProtocolAzure,
					BaseUrl: "https://azure.example",
				},
			},
		}
		got := toInterfacesProviders(in)
		if len(got) != 1 || got[0].ID != "p1" || !got[0].Required {
			t.Fatalf("unexpected: %+v", got)
		}
		if got[0].Current == nil || got[0].Current.APIType != "azure" || got[0].Current.BaseURL != "https://azure.example" {
			t.Errorf("current: %+v", got[0].Current)
		}
		if len(got[0].Supported) != 1 || got[0].Supported[0] != "azure" {
			t.Errorf("supported: %+v", got[0].Supported)
		}
	})
	t.Run("disabled (nil current)", func(t *testing.T) {
		in := []acpsdk.UnstableProviderInfo{{Id: "p2"}}
		got := toInterfacesProviders(in)
		if got[0].Current != nil {
			t.Errorf("expected nil current, got %+v", got[0].Current)
		}
	})
}

// Compile-time check that interfaces.ProviderInfo is the projection type used
// by the Client methods.
var _ = []interfaces.ProviderInfo(nil)
