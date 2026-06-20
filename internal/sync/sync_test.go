package sync

import (
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
