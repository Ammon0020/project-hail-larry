// Package sync implements WebSocket multi-client synchronization.
// Blueprint references: Sec 12 (Multi-Client Synchronization).
//
// The server is authoritative. Connected devices are thin clients rendering
// from the event stream via WebSockets. On reconnect, missing events are synced
// and in-flight permission prompts are re-presented.
package sync

import (
	"context"
	"encoding/json"
	"log"
	"net/http"
	"sync"

	"github.com/adama/local-agent/internal/interfaces"
	"nhooyr.io/websocket"
)

// Hub manages all connected WebSocket clients and broadcasts events to them.
type Hub struct {
	mu      sync.Mutex
	clients map[*Client]bool
}

// Client represents a single connected WebSocket client.
type Client struct {
	conn *websocket.Conn
	hub  *Hub
	send chan []byte
}

// NewHub creates a new WebSocket hub for managing connected clients.
func NewHub() *Hub {
	return &Hub{
		clients: make(map[*Client]bool),
	}
}

// Register adds a client to the hub.
func (h *Hub) Register(client *Client) {
	h.mu.Lock()
	defer h.mu.Unlock()
	h.clients[client] = true
}

// Unregister removes a client from the hub and closes its send channel.
func (h *Hub) Unregister(client *Client) {
	h.mu.Lock()
	defer h.mu.Unlock()
	if _, ok := h.clients[client]; ok {
		delete(h.clients, client)
		close(client.send)
	}
}

// Broadcast sends an event to all connected clients.
func (h *Hub) Broadcast(event interfaces.Event) {
	data, err := json.Marshal(event)
	if err != nil {
		log.Printf("sync: marshal event: %v", err)
		return
	}

	h.mu.Lock()
	clients := make([]*Client, 0, len(h.clients))
	for c := range h.clients {
		clients = append(clients, c)
	}
	h.mu.Unlock()

	for _, client := range clients {
		select {
		case client.send <- data:
		default:
			// Client's send buffer is full — skip this client.
			log.Printf("sync: skipping slow client")
		}
	}
}

// ClientCount returns the number of connected clients.
func (h *Hub) ClientCount() int {
	h.mu.Lock()
	defer h.mu.Unlock()
	return len(h.clients)
}

// HandleWS is the HTTP handler for the /ws WebSocket endpoint.
// It upgrades the connection, registers the client, and pumps messages.
func (h *Hub) HandleWS(w http.ResponseWriter, r *http.Request) {
	// Accept the WebSocket connection.
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		// LAN-only; allow all origins for simplicity in Phase 1.
		InsecureSkipVerify: true,
	})
	if err != nil {
		log.Printf("sync: accept websocket: %v", err)
		return
	}
	// Set a reasonable read limit (1MB).
	conn.SetReadLimit(1 << 20)

	client := &Client{
		conn: conn,
		hub:  h,
		send: make(chan []byte, 64),
	}

	h.Register(client)

	// Start read and write pumps.
	ctx := r.Context()
	go client.writePump(ctx)
	go client.readPump(ctx)
}

// readPump reads messages from the WebSocket connection.
// In Phase 1, clients send permission responses and prompts.
// Messages are handled by the caller via the read loop.
func (c *Client) readPump(ctx context.Context) {
	defer func() {
		c.hub.Unregister(c)
		c.conn.Close(websocket.StatusNormalClosure, "")
	}()

	for {
		_, _, err := c.conn.Read(ctx)
		if err != nil {
			// Client disconnected or error.
			return
		}
		// In Phase 1, we don't process incoming messages from clients here.
		// Permission responses and prompts go through HTTP API endpoints.
	}
}

// writePump sends queued events to the WebSocket client.
func (c *Client) writePump(ctx context.Context) {
	defer c.conn.Close(websocket.StatusNormalClosure, "")

	for {
		select {
		case message, ok := <-c.send:
			if !ok {
				// Channel closed — hub unregistered us.
				return
			}
			if err := c.conn.Write(ctx, websocket.MessageText, message); err != nil {
				return
			}
		case <-ctx.Done():
			return
		}
	}
}
