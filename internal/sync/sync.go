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
	"net"
	"net/http"
	"net/url"
	"strings"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
	"nhooyr.io/websocket"
)

// pingInterval is how often the keepalive goroutine pings each client. A ping
// forces the peer to respond with a pong; if the peer is gone (half-open), the
// ping fails and the connection is closed so its goroutines are reaped instead
// of leaking forever.
const pingInterval = 30 * time.Second

// pingTimeout is the per-ping deadline. If the peer does not respond with a
// pong within this window it is considered dead and the connection is closed.
// It is shorter than pingInterval so a hung peer is detected before the next
// ping is scheduled.
const pingTimeout = 10 * time.Second

// AuthChecker validates a device credential pair. It returns true when the
// deviceID/secret combination belongs to a paired device. The Hub uses it to
// gate WebSocket handshakes so only paired devices can join the event stream.
type AuthChecker func(deviceID, secret string) bool

// Hub manages all connected WebSocket clients and broadcasts events to them.
//
// The Hub owns a lifecycle context (hub.ctx) that is cancelled by Shutdown.
// Every client pump goroutine derives its context from hub.ctx so that a
// graceful daemon shutdown promptly tears down all connections instead of
// leaving goroutines blocked on network reads until the OS times the TCP
// connection out.
type Hub struct {
	mu      sync.Mutex
	clients map[*Client]bool
	auth    AuthChecker

	// ctx is cancelled by Shutdown; pumps select on ctx.Done() to exit.
	ctx       context.Context
	cancelFn  context.CancelFunc
	closeOnce sync.Once
}

// Client represents a single connected WebSocket client.
type Client struct {
	conn *websocket.Conn
	hub  *Hub
	send chan []byte
}

// NewHub creates a new WebSocket hub for managing connected clients. The hub
// creates a lifecycle context that is cancelled by Shutdown; client pump
// goroutines derive from it so they exit promptly on shutdown.
func NewHub() *Hub {
	ctx, cancel := context.WithCancel(context.Background())
	return &Hub{
		clients:  make(map[*Client]bool),
		ctx:      ctx,
		cancelFn: cancel,
	}
}

// Shutdown cancels the hub lifecycle context, causing every connected client's
// read/write pump goroutines to exit and close their WebSocket connections. It
// is safe to call multiple times. The daemon should call this during graceful
// shutdown so connected clients are torn down deterministically rather than
// relying on TCP timeouts.
func (h *Hub) Shutdown() {
	h.closeOnce.Do(func() {
		if h.cancelFn != nil {
			h.cancelFn()
		}
	})
}

// SetAuthChecker wires a credential validator into the hub. When set,
// HandleWS rejects WebSocket handshakes that do not present a valid device
// credential. When nil (e.g. in tests without a pairing manager), auth is
// skipped — in production the daemon always provides a checker.
func (h *Hub) SetAuthChecker(checker AuthChecker) {
	h.auth = checker
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
//
// The hub mutex is held for the entire send loop. This is safe because each
// send uses a non-blocking select (the `default` arm skips full channels), so
// the lock is never held while blocking on a slow client. Holding the lock
// guarantees that Unregister cannot close a client's send channel between the
// moment we snapshot the client set and the moment we send to it, which was
// the source of a send-on-closed-channel panic in the previous lock-free
// implementation.
func (h *Hub) Broadcast(event interfaces.Event) {
	data, err := json.Marshal(event)
	if err != nil {
		log.Printf("sync: marshal event: %v", err)
		return
	}

	h.mu.Lock()
	defer h.mu.Unlock()
	for client := range h.clients {
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
//
// When an AuthChecker is configured, the device credential is extracted from
// the request's query parameters (deviceId and secret) and validated before
// the WebSocket upgrade. Browsers cannot set custom headers on the WebSocket
// handshake, so query params are the only browser-compatible channel. Invalid
// or missing credentials result in a 401 and the connection is refused.
//
// Loopback connections (127.0.0.1, ::1) bypass auth. The daemon runs on the
// host and the host's browser connects via localhost, so the host machine is
// always trusted. Remote (LAN) devices must still present credentials.
//
// CSRF defense: the WebSocket handshake's Origin header is verified before the
// upgrade. Browsers always set Origin on WS handshakes, so an empty Origin is
// rejected (this endpoint is browser-facing). The Origin's host must match the
// request's Host — i.e. the host the client used to reach the daemon. The host
// browser connects via localhost so Origin == Host; a remote paired device
// connects via the LAN IP so Origin == Host there too. A cross-origin
// handshake from a malicious website (e.g. a page on evil.com opening
// ws://localhost:7337/ws from the host browser) carries Origin=evil.com while
// Host=localhost:7337, so it is rejected with 403. Without this check a
// malicious site could open the socket and exfiltrate all IDE data because
// loopback bypasses auth.
func (h *Hub) HandleWS(w http.ResponseWriter, r *http.Request) {
	// Validate device credentials before upgrading the connection.
	// Loopback (host browser) is always allowed.
	if h.auth != nil && !isLoopbackAddr(r.RemoteAddr) {
		deviceID := r.URL.Query().Get("deviceId")
		secret := r.URL.Query().Get("secret")
		if deviceID == "" || secret == "" || !h.auth(deviceID, secret) {
			http.Error(w, "unauthorized", http.StatusUnauthorized)
			return
		}
	}

	// CSRF defense: verify the Origin header before upgrading. See the
	// HandleWS doc comment for the threat model. originAllowed rejects empty
	// Origin (non-browser clients are not expected on this browser-facing
	// endpoint) and any Origin whose host does not match the request Host.
	if !originAllowed(r) {
		http.Error(w, "origin not allowed", http.StatusForbidden)
		return
	}

	// Accept the WebSocket connection. InsecureSkipVerify is left at its
	// default (false) so the library independently re-checks Origin against
	// r.Host as defense in depth — the manual check above is the primary gate
	// and produces a clean 403, while the library check is a backstop.
	conn, err := websocket.Accept(w, r, &websocket.AcceptOptions{
		InsecureSkipVerify: false,
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

	// The pump goroutines derive their lifetime from the hub's context (not
	// r.Context(), which is cancelled when HandleWS returns, and not a
	// locally-cancelled context whose defer would fire on return). Using h.ctx
	// directly means the pumps live until either the hub shuts down or the
	// connection errors out. readPump owns Unregister/cleanup on exit.
	go client.writePump(h.ctx)
	go client.readPump(h.ctx)
}

// readPump reads messages from the WebSocket connection. A keepalive goroutine
// pings the peer every pingInterval; a live peer responds with a pong (which
// the websocket library processes transparently during Read). If the peer
// disappears without closing the TCP connection (network drop, laptop sleep),
// the ping fails and the keepalive closes the connection, causing the pending
// Read to return an error. This reaps both pump goroutines instead of leaking
// them on half-open connections.
//
// nhooyr.io/websocket has no SetReadDeadline method and processes pong control
// frames internally during Read, so a per-read context timeout would falsely
// fire on a live-but-silent connection (the pong resets liveness but does not
// reset a context deadline). Liveness is therefore enforced solely via the
// keepalive pings, which is the pattern recommended for this library.
//
// In Phase 1, clients send permission responses and prompts. Messages are
// handled by the caller via the read loop.
func (c *Client) readPump(ctx context.Context) {
	defer func() {
		c.hub.Unregister(c)
		_ = c.conn.Close(websocket.StatusNormalClosure, "")
	}()

	// Keepalive: ping the peer on a ticker. A failed ping means the peer is
	// gone, so we close the connection to unblock the pending Read below.
	pingCtx, pingCancel := context.WithCancel(ctx)
	defer pingCancel()
	go c.keepalive(pingCtx)

	for {
		_, _, err := c.conn.Read(ctx)
		if err != nil {
			// Client disconnected, keepalive closed the conn, or hub shutdown.
			return
		}
		// In Phase 1, we don't process incoming messages from clients here.
		// Permission responses and prompts go through HTTP API endpoints.
	}
}

// keepalive periodically pings the peer. A ping elicits a pong which the
// websocket library processes during Read, resetting the idle window. If the
// ping itself fails (peer gone), the connection is closed so the readPump's
// pending Read returns an error and both goroutines exit. The goroutine exits
// when ctx is cancelled (hub shutdown or readPump returning).
func (c *Client) keepalive(ctx context.Context) {
	ticker := time.NewTicker(pingInterval)
	defer ticker.Stop()
	for {
		select {
		case <-ticker.C:
			// Ping with a short per-ping timeout so a hung peer is detected
			// before the next ping is scheduled.
			pingCtx, cancel := context.WithTimeout(ctx, pingTimeout)
			if err := c.conn.Ping(pingCtx); err != nil {
				cancel()
				// Ping failed — peer is gone. Close the connection to unblock
				// readPump's Read.
				_ = c.conn.Close(websocket.StatusPolicyViolation, "keepalive ping failed")
				return
			}
			cancel()
		case <-ctx.Done():
			return
		}
	}
}

// writePump sends queued events to the WebSocket client.
func (c *Client) writePump(ctx context.Context) {
	defer func() { _ = c.conn.Close(websocket.StatusNormalClosure, "") }()

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

// isLoopbackAddr reports whether the given RemoteAddr (host:port form) refers
// to a loopback address (127.0.0.1 or ::1). The host machine's browser
// connects via localhost, so such connections are trusted and bypass device
// credential checks.
func isLoopbackAddr(remoteAddr string) bool {
	host, _, err := net.SplitHostPort(remoteAddr)
	if err != nil {
		// No port; treat the whole string as the host.
		host = remoteAddr
	}
	return host == "127.0.0.1" || host == "::1" || host == "localhost"
}

// originAllowed reports whether the WebSocket upgrade request's Origin header
// is acceptable as a CSRF defense. Browsers always set Origin on WebSocket
// handshakes, so an empty Origin is rejected — this endpoint is browser-facing
// and a missing Origin is suspicious. The Origin's host must match the
// request's Host (the host the client used to reach the daemon): the host
// browser connects via localhost so Origin == Host, and a remote paired device
// connects via the LAN IP so Origin == Host there as well. A cross-origin
// handshake from a malicious website has Origin != Host and is rejected,
// preventing it from opening ws://localhost:<port>/ws and exfiltrating IDE
// data through the loopback auth bypass.
func originAllowed(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	if origin == "" {
		return false
	}
	u, err := url.Parse(origin)
	if err != nil || u.Host == "" {
		return false
	}
	return strings.EqualFold(u.Host, r.Host)
}
