package sync

import (
	"net/http"
	"net/http/httptest"
	"testing"

	"github.com/adama/local-agent/internal/interfaces"
)

// TestHubRegisterUnregister verifies client registration lifecycle.
func TestHubRegisterUnregister(t *testing.T) {
	hub := NewHub()

	if hub.ClientCount() != 0 {
		t.Errorf("expected 0 clients, got %d", hub.ClientCount())
	}

	client := &Client{
		hub:  hub,
		send: make(chan []byte, 1),
	}

	hub.Register(client)
	if hub.ClientCount() != 1 {
		t.Errorf("expected 1 client, got %d", hub.ClientCount())
	}

	hub.Unregister(client)
	if hub.ClientCount() != 0 {
		t.Errorf("expected 0 clients after unregister, got %d", hub.ClientCount())
	}
}

// TestBroadcast verifies that events are sent to all connected clients.
func TestBroadcast(t *testing.T) {
	hub := NewHub()

	// Create two mock clients.
	c1 := &Client{hub: hub, send: make(chan []byte, 1)}
	c2 := &Client{hub: hub, send: make(chan []byte, 1)}

	hub.Register(c1)
	hub.Register(c2)

	// Broadcast an event.
	hub.Broadcast(interfaces.Event{
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "s1",
		Content:   "hello",
	})

	// Both clients should receive the event.
	select {
	case msg := <-c1.send:
		if len(msg) == 0 {
			t.Error("expected non-empty message for client 1")
		}
	default:
		t.Error("client 1 did not receive message")
	}

	select {
	case msg := <-c2.send:
		if len(msg) == 0 {
			t.Error("expected non-empty message for client 2")
		}
	default:
		t.Error("client 2 did not receive message")
	}
}

// TestBroadcastNoClients verifies broadcast doesn't panic with no clients.
func TestBroadcastNoClients(_ *testing.T) {
	hub := NewHub()

	// Should not panic.
	hub.Broadcast(interfaces.Event{
		Type:      interfaces.EventPromptSubmitted,
		SessionID: "s1",
	})
}

// TestOriginAllowed verifies the WebSocket Origin CSRF check. Browsers always
// send Origin on WS handshakes; the header's host must match the request Host
// (the host the client used to reach the daemon). An empty Origin is rejected
// because this endpoint is browser-facing, and a cross-origin Origin (e.g. a
// malicious website opening ws://localhost:7337/ws) is rejected because its
// host differs from the request Host.
func TestOriginAllowed(t *testing.T) {
	cases := []struct {
		name   string
		host   string
		origin string
		want   bool
	}{
		{"same-origin localhost", "localhost:7337", "http://localhost:7337", true},
		{"same-origin 127.0.0.1", "127.0.0.1:7337", "http://127.0.0.1:7337", true},
		{"same-origin LAN ip", "192.168.1.5:7337", "http://192.168.1.5:7337", true},
		{"cross-origin attacker", "localhost:7337", "http://evil.com", false},
		{"cross-origin attacker with port", "localhost:7337", "http://evil.com:80", false},
		{"empty origin", "localhost:7337", "", false},
		{"origin host mismatched port", "localhost:7337", "http://localhost:9999", false},
		{"malformed origin", "localhost:7337", "://bad", false},
		{"origin without host", "localhost:7337", "http://", false},
	}
	for _, tc := range cases {
		t.Run(tc.name, func(t *testing.T) {
			req := httptest.NewRequest(http.MethodGet, "http://"+tc.host+"/ws", nil)
			if tc.origin != "" {
				req.Header.Set("Origin", tc.origin)
			}
			if got := originAllowed(req); got != tc.want {
				t.Errorf("originAllowed() = %v, want %v", got, tc.want)
			}
		})
	}
}

// TestHandleWSRejectsCrossOrigin verifies HandleWS rejects a cross-origin
// WebSocket handshake with 403 before attempting the upgrade. The request is
// loopback (so auth is bypassed) but its Origin points at an attacker domain,
// which must be refused to prevent a malicious website from opening
// ws://localhost:7337/ws and exfiltrating IDE data. The 403 is written before
// websocket.Accept is reached, so a plain httptest recorder suffices.
func TestHandleWSRejectsCrossOrigin(t *testing.T) {
	hub := NewHub()
	// Configure an auth checker to prove the Origin gate runs even when auth
	// would otherwise be bypassed for loopback.
	hub.SetAuthChecker(func(_, _ string) bool { return false })

	req := httptest.NewRequest(http.MethodGet, "http://localhost:7337/ws", nil)
	req.RemoteAddr = "127.0.0.1:1234" // loopback -> auth bypassed
	req.Header.Set("Origin", "http://evil.com")
	rec := httptest.NewRecorder()

	hub.HandleWS(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Errorf("expected 403 for cross-origin WS handshake, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestHandleWSRejectsEmptyOrigin verifies HandleWS rejects a WebSocket
// handshake with no Origin header (browser-facing endpoint requires it) with
// 403, even for loopback connections that bypass device auth.
func TestHandleWSRejectsEmptyOrigin(t *testing.T) {
	hub := NewHub()

	req := httptest.NewRequest(http.MethodGet, "http://localhost:7337/ws", nil)
	req.RemoteAddr = "127.0.0.1:1234" // loopback -> auth bypassed
	// No Origin header set.
	rec := httptest.NewRecorder()

	hub.HandleWS(rec, req)

	if rec.Code != http.StatusForbidden {
		t.Errorf("expected 403 for empty Origin, got %d (body: %s)", rec.Code, rec.Body.String())
	}
}

// TestHandleWSRejectsNonLoopbackNoCred verifies HandleWS still enforces device
// auth for non-loopback connections (returning 401) — the Origin check must not
// weaken the existing remote-auth gate.
func TestHandleWSRejectsNonLoopbackNoCred(t *testing.T) {
	hub := NewHub()
	hub.SetAuthChecker(func(_, _ string) bool { return false })

	req := httptest.NewRequest(http.MethodGet, "http://localhost:7337/ws", nil)
	req.RemoteAddr = "192.168.1.10:1234" // non-loopback -> must auth
	req.Header.Set("Origin", "http://localhost:7337")
	rec := httptest.NewRecorder()

	hub.HandleWS(rec, req)

	if rec.Code != http.StatusUnauthorized {
		t.Errorf("expected 401 for non-loopback without credentials, got %d", rec.Code)
	}
}
