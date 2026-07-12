// Package server provides the HTTP server that serves the web UI and API.
// It embeds the frontend build via go:embed and serves it in production.
// Blueprint references: Sec 3 (Architecture), Sec 25 (Phase 1).
package server

import (
	"context"
	"embed"
	"encoding/json"
	"errors"
	"fmt"
	"io/fs"
	"log"
	"net"
	"net/http"
	"net/url"
	"reflect"
	"strings"
	"time"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/config"
	"github.com/adama/local-agent/internal/events"
	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/pairing"
	"github.com/adama/local-agent/internal/permissions"
	"github.com/adama/local-agent/internal/sync"
	"github.com/adama/local-agent/internal/uploads"
	"github.com/adama/local-agent/internal/workspace"
)

//go:embed all:dist
var frontendFS embed.FS

// Shared constants for the server package. localhost is the loopback hostname
// used by host checks and TLS SANs; statusKey/statusUpdated are JSON keys in
// the generic status response envelope written by several API handlers.
const (
	localhost     = "localhost"
	statusKey     = "status"
	statusUpdated = "updated"
)

// Deps holds all the manager dependencies the server needs.
type Deps struct {
	EventStore       *events.Store
	PairingMgr       *pairing.Manager
	WorkspaceMgr     *workspace.Manager
	ACPClient        *acp.Client
	PermissionMgr    *permissions.Manager
	SyncHub          *sync.Hub
	Config           *config.Config
	OpenFilesTracker *acp.OpenFilesTracker
	Uploads          *uploads.Manager
	// McpConfigPath is the path to mcp.json (typically
	// ~/.local-agent/mcp.json). The /api/mcp endpoints read/write this file;
	// the ACP client loads it at session start. Empty disables the MCP REST
	// API (handlers return 503) and the client passes no MCP servers.
	McpConfigPath string
}

// Server is the main HTTP server for the Local Agent Interface.
type Server struct {
	mux        *http.ServeMux
	deps       *Deps
	tlsEnabled bool
	certPath   string
	keyPath    string
	// httpServer holds the active *http.Server once ListenAndServe (or
	// ListenDual) has been called. It is stored so Shutdown can gracefully
	// drain in-flight HTTP requests during daemon teardown.
	httpServer *http.Server
	// httpsServer holds the active TLS *http.Server when ListenDual has
	// started the HTTPS listener. It is nil when running HTTP-only (or when
	// ListenAndServeTLS was used directly for the single-server TLS path).
	// Shutdown drains both httpServer and httpsServer so dual-mode teardown
	// does not leave the TLS listener accepting new connections.
	httpsServer *http.Server
}

// New creates a new Server with the given dependencies.
// If deps is nil, only health check and frontend serving are enabled.
func New(deps *Deps) *Server {
	s := &Server{
		mux:  http.NewServeMux(),
		deps: deps,
	}
	if deps != nil && deps.ACPClient != nil {
		deps.ACPClient.SetCallbacks(s)
	}
	if deps != nil && deps.PermissionMgr != nil {
		deps.PermissionMgr.SetCallback(s.onPermissionRequested)
	}
	s.routes()
	return s
}

// onPermissionRequested is invoked by the PermissionManager when an agent
// requests permission. It persists and broadcasts a PermissionRequested event so
// every connected device can prompt the user.
func (s *Server) onPermissionRequested(req interfaces.PermissionRequest) {
	options := make([]string, 0, len(req.Options))
	for _, o := range req.Options {
		options = append(options, string(o))
	}
	s.recordEvent(context.Background(), interfaces.Event{
		Type:      interfaces.EventPermissionRequested,
		SessionID: req.SessionID,
		RequestID: req.ID,
		Tool:      req.Tool,
		Command:   req.Command,
		Target:    req.Target,
		Options:   options,
	})
}

// routes sets up all HTTP routes.
func (s *Server) routes() {
	// Health check.
	s.mux.HandleFunc("GET /health", s.handleHealth)

	// API routes (only if deps are provided).
	if s.deps != nil {
		s.apiRoutes()
	}

	// Serve embedded frontend.
	s.serveFrontend()
}

// apiRoutes registers all /api/* and /ws routes.
//
// Pairing endpoints (/api/pair/*) are registered without authentication so
// unpaired devices can initiate pairing. Every other /api/ route is wrapped
// in requireAuth, which validates the device credential presented in the
// Authorization header or query params against the PairingManager. The
// WebSocket hub receives an AuthChecker so HandleWS can gate handshakes the
// same way.
func (s *Server) apiRoutes() {
	d := s.deps

	// Pairing routes — no auth (devices are not yet paired), but rate-limited.
	s.mux.HandleFunc("POST /api/pair/initiate", withPairRateLimit(s.handlePairInitiate))
	s.mux.HandleFunc("POST /api/pair/verify-passcode", withPairRateLimit(s.handlePairVerifyPasscode))
	s.mux.HandleFunc("POST /api/pair/verify-token", withPairRateLimit(s.handlePairVerifyToken))

	// Device management routes — require auth.
	s.mux.HandleFunc("GET /api/devices", s.requireAuth(s.handleListDevices))
	s.mux.HandleFunc("DELETE /api/devices/{id}", s.requireAuth(s.handleRevokeDevice))

	// Workspace routes — require auth.
	s.mux.HandleFunc("GET /api/workspaces", s.requireAuth(s.handleListWorkspaces))
	s.mux.HandleFunc("POST /api/workspaces", s.requireAuth(s.handleRegisterWorkspace))
	s.mux.HandleFunc("GET /api/workspaces/{id}/files", s.requireAuth(s.handleFileTree))
	s.mux.HandleFunc("GET /api/workspaces/{id}/file", s.requireAuth(s.handleReadFile))
	s.mux.HandleFunc("POST /api/workspaces/{id}/file", s.requireAuth(s.handleWriteFile))
	s.mux.HandleFunc("GET /api/workspaces/{id}/search", s.requireAuth(s.handleSearch))

	// Event routes — require auth.
	s.mux.HandleFunc("GET /api/events", s.requireAuth(s.handleGetEvents))
	s.mux.HandleFunc("GET /api/events/{sessionId}", s.requireAuth(s.handleGetSessionEvents))

	// Session routes — require auth.
	s.mux.HandleFunc("GET /api/agents", s.requireAuth(s.handleListAgents))
	s.mux.HandleFunc("POST /api/agents", s.requireAuth(s.handleUpsertAgent))
	s.mux.HandleFunc("DELETE /api/agents/{id}", s.requireAuth(s.handleDeleteAgent))
	s.mux.HandleFunc("POST /api/agents/autodetect", s.requireAuth(s.handleAutodetectAgents))
	s.mux.HandleFunc("GET /api/sessions", s.requireAuth(s.handleListSessions))
	s.mux.HandleFunc("GET /api/sessions/{id}", s.requireAuth(s.handleGetSession))
	s.mux.HandleFunc("GET /api/sessions/{id}/export", s.requireAuth(s.handleExportSession))
	s.mux.HandleFunc("POST /api/sessions", s.requireAuth(s.handleCreateSession))
	s.mux.HandleFunc("PATCH /api/sessions/{id}", s.requireAuth(s.handlePatchSession))
	s.mux.HandleFunc("POST /api/sessions/{id}/prompt", s.requireAuth(s.handleSendPrompt))
	s.mux.HandleFunc("POST /api/sessions/{id}/cancel", s.requireAuth(s.handleCancelSession))
	s.mux.HandleFunc("DELETE /api/sessions/{id}", s.requireAuth(s.handleCloseSession))
	s.mux.HandleFunc("POST /api/sessions/{id}/context", s.requireAuth(s.handleSessionContext))
	s.mux.HandleFunc("POST /api/sessions/{id}/uploads", s.requireAuth(s.handleUpload))
	s.mux.HandleFunc("GET /api/sessions/{id}/uploads/{uploadID}", s.requireAuth(s.handleServeUpload))

	// ACP provider management routes — require auth. Expose the unstable
	// providers/list|set|disable methods per live session transport.
	s.mux.HandleFunc("GET /api/sessions/{id}/providers", s.requireAuth(s.handleListProviders))
	s.mux.HandleFunc("PUT /api/sessions/{id}/providers/{providerId}", s.requireAuth(s.handleSetProvider))
	s.mux.HandleFunc("DELETE /api/sessions/{id}/providers/{providerId}", s.requireAuth(s.handleDisableProvider))

	// Permission routes — require auth.
	s.mux.HandleFunc("GET /api/permissions/pending", s.requireAuth(s.handlePendingPermissions))
	s.mux.HandleFunc("POST /api/permissions/{id}/respond", s.requireAuth(s.handleRespondPermission))

	// MCP server config routes — require auth. The MCP config is a separate
	// mcp.json file (Claude Desktop–compatible) edited as raw JSON by the
	// frontend so formatting/comments survive round-trips.
	s.mux.HandleFunc("GET /api/mcp", s.requireAuth(s.handleGetMcp))
	s.mux.HandleFunc("PUT /api/mcp", s.requireAuth(s.handlePutMcp))
	s.mux.HandleFunc("PATCH /api/mcp/servers/{name}", s.requireAuth(s.handlePatchMcpServer))
	s.mux.HandleFunc("GET /api/mcp/status", s.requireAuth(s.handleGetMcpStatus))

	// WebSocket endpoint — auth is enforced inside HandleWS via the hub's
	// AuthChecker (browsers cannot set headers on the WS handshake, so the
	// credential is passed as query params instead).
	if d.SyncHub != nil {
		if d.PairingMgr != nil {
			d.SyncHub.SetAuthChecker(d.PairingMgr.ValidateCredential)
		}
		s.mux.HandleFunc("/ws", d.SyncHub.HandleWS)
	}
}

// requireAuth wraps an http.HandlerFunc with device credential validation.
// Requests must present a valid device credential via an Authorization:
// "Bearer <deviceId>:<secret>" header or deviceId/secret query params. When
// PairingMgr is nil (degraded/test setup with partial deps), auth is skipped
// — in production the daemon always wires a PairingManager.
//
// Loopback connections (127.0.0.1, ::1, localhost) bypass auth. The daemon
// runs on the host and the host's browser accesses it via localhost, so the
// host machine always has full access without needing to present device
// credentials. Remote (LAN) devices still must authenticate. This keeps the
// frontend working before it is updated to send credentials, and preserves
// security for non-loopback callers once it is.
//
// CSRF defense for the loopback bypass: a malicious website opened in the host
// browser can issue cross-origin POST/PUT/PATCH/DELETE requests to
// http://localhost:<port>/api/... that the browser sends with the attacker's
// Origin. Browsers always set Origin on such requests, so for mutating methods
// on loopback we require the Origin — when present — to be a loopback origin
// (localhost / 127.0.0.1 / ::1). Non-browser clients (the CLI, curl) typically
// omit Origin and are still allowed, since they are not CSRF vectors.
// Read-only GET/HEAD/OPTIONS requests are exempt: the browser same-origin
// policy already hides cross-origin response bodies, and the CLI issues GETs
// without Origin.
func (s *Server) requireAuth(next http.HandlerFunc) http.HandlerFunc {
	return func(w http.ResponseWriter, r *http.Request) {
		if s.deps == nil || s.deps.PairingMgr == nil {
			next(w, r)
			return
		}
		if isLoopback(r) {
			if isMutatingMethod(r.Method) && !loopbackOriginAllowed(r) {
				writeError(w, http.StatusForbidden, "cross-origin request not allowed")
				return
			}
			next(w, r)
			return
		}
		if !s.authenticate(r) {
			writeError(w, http.StatusUnauthorized, "unauthorized")
			return
		}
		next(w, r)
	}
}

// isMutatingMethod reports whether the HTTP method can change server state.
// GET/HEAD/OPTIONS are read-only; POST/PUT/PATCH/DELETE are mutating and are
// the CSRF-relevant methods for the loopback auth bypass.
func isMutatingMethod(method string) bool {
	switch method {
	case http.MethodPost, http.MethodPut, http.MethodPatch, http.MethodDelete:
		return true
	}
	return false
}

// loopbackOriginAllowed validates the Origin header on a loopback request as a
// CSRF defense. It returns true when the request is not browser-initiated (no
// Origin header — e.g. the CLI or curl, which are not CSRF vectors) OR when
// the Origin is a loopback origin (localhost, 127.0.0.1, or ::1 on any port),
// which is what the host browser uses. A cross-origin request from a malicious
// website carries the attacker's Origin and is rejected. The Origin host is
// compared (port-stripped) so any local port is accepted.
func loopbackOriginAllowed(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	if origin == "" {
		// Non-browser client (no Origin). Not a CSRF vector — allow.
		return true
	}
	u, err := url.Parse(origin)
	if err != nil || u.Host == "" {
		return false
	}
	host := u.Hostname()
	return host == "127.0.0.1" || host == "::1" || host == localhost
}

// isLoopback reports whether the request originated from a loopback address
// (127.0.0.1 or ::1). http.Request.RemoteAddr is of the form "host:port"
// (or "[host]:port" for IPv6), so net.SplitHostPort is used to extract the
// host. The host machine's browser connects via localhost, so such requests
// are trusted.
func isLoopback(r *http.Request) bool {
	host, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		// RemoteAddr had no port; treat the whole string as the host.
		host = r.RemoteAddr
	}
	return host == "127.0.0.1" || host == "::1" || host == localhost
}

// authenticate extracts the device credential from the request and validates
// it against the pairing manager. It checks the Authorization header first
// (Bearer <deviceId>:<secret>), then falls back to deviceId/secret query
// params (needed for WebSocket and other browser-initiated requests that
// cannot set custom headers).
func (s *Server) authenticate(r *http.Request) bool {
	deviceID, secret := extractCredential(r)
	if deviceID == "" || secret == "" {
		return false
	}
	return s.deps.PairingMgr.ValidateCredential(deviceID, secret)
}

// extractCredential pulls the device ID and secret from either the
// Authorization header ("Bearer <deviceId>:<secret>") or the deviceId/secret
// query parameters. Returns empty strings when no credential is present.
func extractCredential(r *http.Request) (deviceID, secret string) {
	// Prefer the Authorization header.
	if auth := r.Header.Get("Authorization"); strings.HasPrefix(auth, "Bearer ") {
		token := strings.TrimPrefix(auth, "Bearer ")
		if idx := strings.IndexByte(token, ':'); idx > 0 {
			return token[:idx], token[idx+1:]
		}
	}
	// Fall back to query params (browser WebSocket/SSE cannot set headers).
	return r.URL.Query().Get("deviceId"), r.URL.Query().Get("secret")
}

// handleHealth responds with a simple JSON health check.
func (s *Server) handleHealth(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{statusKey: "ok"})
}

// serveFrontend sets up the embedded React build as static files.
func (s *Server) serveFrontend() {
	distFS, err := fs.Sub(frontendFS, "dist")
	if err != nil {
		log.Printf("WARNING: frontend dist not embedded: %v", err)
		return
	}

	fileServer := http.FileServer(http.FS(distFS))

	s.mux.Handle("GET /assets/", fileServer)

	// SPA fallback: any non-API route serves index.html.
	s.mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/api/") || strings.HasPrefix(r.URL.Path, "/ws") {
			http.NotFound(w, r)
			return
		}

		path := r.URL.Path
		if path == "/" {
			path = "/index.html"
		}

		if _, err := fs.Stat(distFS, strings.TrimPrefix(path, "/")); err == nil {
			fileServer.ServeHTTP(w, r)
			return
		}

		r.URL.Path = "/"
		fileServer.ServeHTTP(w, r)
	})
}

// newHTTPServer constructs an *http.Server with sane timeout defaults that
// mitigate slowloris-style resource exhaustion:
//   - ReadHeaderTimeout: 5s  — drop connections that don't send headers fast.
//   - ReadTimeout:       30s — cap the total time to read the request.
//   - WriteTimeout:      60s — cap the total time to write the response.
//   - IdleTimeout:       120s — close keep-alive connections that go idle.
//
// WriteTimeout does not affect the WebSocket endpoint (/ws) or any other
// hijacked connection — once upgraded, the http.Server timeouts no longer
// apply to that connection.
func newHTTPServer(addr string, handler http.Handler) *http.Server {
	return &http.Server{
		Addr:              addr,
		Handler:           handler,
		ReadHeaderTimeout: 5 * time.Second,
		ReadTimeout:       30 * time.Second,
		WriteTimeout:      60 * time.Second,
		IdleTimeout:       120 * time.Second,
	}
}

// ListenAndServe starts the HTTP server on the given address.
// If TLS is enabled (via SetTLS), it serves over HTTPS using the configured
// certificate and key paths. The underlying *http.Server is stored on s so
// that Shutdown can gracefully drain in-flight requests.
//
// This is the single-listener path used by HTTP-only tests and the legacy
// single-TLS path. The daemon uses ListenDual for simultaneous HTTP+HTTPS.
func (s *Server) ListenAndServe(addr string) error {
	if s.tlsEnabled {
		return s.ListenAndServeTLS(addr, s.certPath, s.keyPath)
	}
	log.Printf("Server listening on http://%s", addr)

	s.httpServer = newHTTPServer(addr, s.mux)
	return s.httpServer.ListenAndServe()
}

// ListenAndServeTLS starts the HTTPS server on the given address using the
// provided certificate and key paths. The underlying *http.Server is stored
// on s so that Shutdown can gracefully drain in-flight requests.
func (s *Server) ListenAndServeTLS(addr, certPath, keyPath string) error {
	log.Printf("Server listening on https://%s", addr)

	s.httpServer = newHTTPServer(addr, s.mux)
	return s.httpServer.ListenAndServeTLS(certPath, keyPath)
}

// ListenDual starts two listeners simultaneously: a cleartext HTTP listener on
// httpAddr and a TLS HTTPS listener on httpsAddr using the provided cert and
// key paths. This lets a user pick a scheme by typing http://IP:port or
// https://IP:httpsPort in the browser without restarting the daemon or
// flipping config.
//
// Both listeners serve the same mux (same handlers, same auth, same WS hub).
// Each runs in its own goroutine. The first non-nil error returned by either
// listener is forwarded on the returned channel — except http.ErrServerClosed,
// which is the normal Shutdown path and is suppressed. A bind failure on
// either listener is reported (fail-fast): if the HTTPS listener cannot bind,
// the HTTP listener is shut down and the HTTPS error is returned so the user
// is not silently left with only cleartext when they requested dual mode.
//
// The caller should run this in a goroutine and select on the returned channel
// (alongside a shutdown context) to detect a listener dying unexpectedly:
//
//	errCh := srv.ListenDual(httpAddr, httpsAddr, cert, key)
//	select {
//	case err := <-errCh:
//	    ...
//	case <-ctx.Done():
//	    _ = srv.Shutdown(shutdownCtx)
//	}
func (s *Server) ListenDual(httpAddr, httpsAddr, certPath, keyPath string) <-chan error {
	log.Printf("Server listening on http://%s", httpAddr)
	log.Printf("Server listening on https://%s", httpsAddr)

	s.httpServer = newHTTPServer(httpAddr, s.mux)
	s.httpsServer = newHTTPServer(httpsAddr, s.mux)

	errCh := make(chan error, 2)

	// HTTP goroutine.
	go func() {
		err := s.httpServer.ListenAndServe()
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			errCh <- fmt.Errorf("http listener %s: %w", httpAddr, err)
			return
		}
		errCh <- nil
	}()

	// HTTPS goroutine. On bind failure, drain the HTTP listener too so the
	// caller does not end up with a half-up dual stack when it requested both.
	go func() {
		err := s.httpsServer.ListenAndServeTLS(certPath, keyPath)
		if err != nil && !errors.Is(err, http.ErrServerClosed) {
			errCh <- fmt.Errorf("https listener %s: %w", httpsAddr, err)
			// Best-effort: stop the HTTP listener so the daemon returns rather
			// than hanging on the still-alive HTTP goroutine.
			shutCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
			defer cancel()
			_ = s.httpServer.Shutdown(shutCtx)
			return
		}
		errCh <- nil
	}()

	return errCh
}

// Shutdown gracefully shuts down the HTTP (and, in dual mode, HTTPS) server,
// waiting for in-flight requests to complete or until the context is
// cancelled. It is safe to call when the server was never started (it returns
// nil). The daemon calls this during signal-handled teardown before closing
// the EventStore so that in-flight handlers do not Append to a closed store.
//
// In dual mode both listeners are drained; the first shutdown error is
// returned but both are always attempted.
func (s *Server) Shutdown(ctx context.Context) error {
	// Drain both listeners. The first non-nil error is returned, but both are
	// always attempted so a stuck HTTP shutdown does not skip the HTTPS drain
	// (and vice versa). firstErr is assigned via a helper to avoid a govet
	// nilness false-positive on the inline `&& firstErr == nil` check.
	var firstErr error
	setFirst := func(err error) {
		if err != nil && firstErr == nil {
			firstErr = err
		}
	}
	if s.httpServer != nil {
		setFirst(s.httpServer.Shutdown(ctx))
	}
	if s.httpsServer != nil {
		setFirst(s.httpsServer.Shutdown(ctx))
	}
	return firstErr
}

// SetTLS enables TLS for the server. When enabled, ListenAndServe will serve
// over HTTPS using the provided certificate and key paths. The certificate
// and key paths should be set before calling ListenAndServe.
func (s *Server) SetTLS(certPath, keyPath string) {
	s.tlsEnabled = true
	s.certPath = certPath
	s.keyPath = keyPath
}

// Handler returns the http.Handler for testing.
func (s *Server) Handler() http.Handler {
	return s.mux
}

// OnEvent receives ACP client events and publishes them to connected clients.
func (s *Server) OnEvent(event interfaces.Event) {
	s.recordEvent(context.Background(), event)
}

// recordEvent persists an event to the event store and broadcasts it via
// WebSocket. It is a no-op when EventStore is nil.
func (s *Server) recordEvent(ctx context.Context, e interfaces.Event) {
	if s.deps == nil || s.deps.EventStore == nil {
		return
	}
	event, err := s.deps.EventStore.Append(ctx, e)
	if err != nil {
		log.Printf("server: record event: %v", err)
		return
	}
	if s.deps.SyncHub != nil {
		s.deps.SyncHub.Broadcast(event)
	}
}

// writeJSON writes a JSON response with the given status code.
// Nil slices are converted to empty slices so they serialize as [] not null.
func writeJSON(w http.ResponseWriter, code int, v interface{}) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(code)
	if v != nil {
		rv := reflect.ValueOf(v)
		if rv.Kind() == reflect.Slice && rv.IsNil() {
			v = reflect.MakeSlice(rv.Type(), 0, 0).Interface()
		}
	}
	if err := json.NewEncoder(w).Encode(v); err != nil {
		log.Printf("write json: %v", err)
	}
}

// writeError writes a JSON error response.
func writeError(w http.ResponseWriter, code int, msg string) {
	writeJSON(w, code, map[string]string{"error": msg})
}

// defaultMaxBodyBytes is the default request body size limit (10 MB) applied
// by decodeJSON. It is large enough for typical JSON payloads while preventing
// a single request from exhausting memory. Endpoints that legitimately accept
// larger bodies (e.g. file writes) use decodeJSONLimit with a higher cap.
const defaultMaxBodyBytes int64 = 10 << 20

// decodeJSON decodes a JSON request body into v, enforcing the default body
// size limit (10 MB) via http.MaxBytesReader. Callers that need a higher limit
// (e.g. file-write endpoints) should use decodeJSONLimit instead.
func decodeJSON(w http.ResponseWriter, r *http.Request, v interface{}) error {
	return decodeJSONLimit(w, r, v, defaultMaxBodyBytes)
}

// decodeJSONLimit decodes a JSON request body into v, enforcing the given
// maxBytes limit via http.MaxBytesReader. The ResponseWriter is required so
// MaxBytesReader can close the connection if the client exceeds the limit,
// preventing it from continuing to stream data.
func decodeJSONLimit(w http.ResponseWriter, r *http.Request, v interface{}, maxBytes int64) error {
	defer func() { _ = r.Body.Close() }()
	r.Body = http.MaxBytesReader(w, r.Body, maxBytes)
	return json.NewDecoder(r.Body).Decode(v)
}
