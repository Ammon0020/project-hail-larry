// Package server provides the HTTP server that serves the web UI and API.
// It embeds the frontend build via go:embed and serves it in production.
// Blueprint references: Sec 3 (Architecture), Sec 25 (Phase 1).
package server

import (
	"context"
	"embed"
	"encoding/json"
	"io/fs"
	"log"
	"net/http"
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
	"github.com/adama/local-agent/internal/workspace"
)

//go:embed all:dist
var frontendFS embed.FS

// Deps holds all the manager dependencies the server needs.
type Deps struct {
	EventStore    *events.Store
	PairingMgr    *pairing.Manager
	WorkspaceMgr  *workspace.Manager
	ACPClient     *acp.Client
	PermissionMgr *permissions.Manager
	SyncHub       *sync.Hub
	Config        *config.Config
}

// Server is the main HTTP server for the Local Agent Interface.
type Server struct {
	mux  *http.ServeMux
	deps *Deps
}

// New creates a new Server with the given dependencies.
// If deps is nil, only health check and frontend serving are enabled.
func New(deps *Deps) *Server {
	s := &Server{
		mux:  http.NewServeMux(),
		deps: deps,
	}
	s.routes()
	return s
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
func (s *Server) apiRoutes() {
	d := s.deps

	// Pairing routes.
	s.mux.HandleFunc("POST /api/pair/initiate", s.handlePairInitiate)
	s.mux.HandleFunc("POST /api/pair/verify-passcode", s.handlePairVerifyPasscode)
	s.mux.HandleFunc("POST /api/pair/verify-token", s.handlePairVerifyToken)
	s.mux.HandleFunc("GET /api/devices", s.handleListDevices)
	s.mux.HandleFunc("DELETE /api/devices/{id}", s.handleRevokeDevice)

	// Workspace routes.
	s.mux.HandleFunc("GET /api/workspaces", s.handleListWorkspaces)
	s.mux.HandleFunc("POST /api/workspaces", s.handleRegisterWorkspace)
	s.mux.HandleFunc("GET /api/workspaces/{id}/files", s.handleFileTree)
	s.mux.HandleFunc("GET /api/workspaces/{id}/file", s.handleReadFile)
	s.mux.HandleFunc("POST /api/workspaces/{id}/file", s.handleWriteFile)

	// Event routes.
	s.mux.HandleFunc("GET /api/events", s.handleGetEvents)
	s.mux.HandleFunc("GET /api/events/{sessionId}", s.handleGetSessionEvents)

	// Session routes.
	s.mux.HandleFunc("GET /api/agents", s.handleListAgents)
	s.mux.HandleFunc("POST /api/agents", s.handleUpsertAgent)
	s.mux.HandleFunc("DELETE /api/agents/{id}", s.handleDeleteAgent)
	s.mux.HandleFunc("POST /api/agents/autodetect", s.handleAutodetectAgents)
	s.mux.HandleFunc("GET /api/sessions", s.handleListSessions)
	s.mux.HandleFunc("POST /api/sessions", s.handleCreateSession)
	s.mux.HandleFunc("POST /api/sessions/{id}/prompt", s.handleSendPrompt)
	s.mux.HandleFunc("POST /api/sessions/{id}/cancel", s.handleCancelSession)
	s.mux.HandleFunc("DELETE /api/sessions/{id}", s.handleCloseSession)

	// Permission routes.
	s.mux.HandleFunc("GET /api/permissions/pending", s.handlePendingPermissions)
	s.mux.HandleFunc("POST /api/permissions/{id}/respond", s.handleRespondPermission)

	// WebSocket endpoint.
	if d.SyncHub != nil {
		s.mux.HandleFunc("/ws", d.SyncHub.HandleWS)
	}
}

// handleHealth responds with a simple JSON health check.
func (s *Server) handleHealth(w http.ResponseWriter, _ *http.Request) {
	writeJSON(w, http.StatusOK, map[string]string{"status": "ok"})
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

// ListenAndServe starts the HTTP server on the given address.
func (s *Server) ListenAndServe(addr string) error {
	log.Printf("Server listening on %s", addr)

	httpServer := &http.Server{
		Addr:              addr,
		Handler:           s.mux,
		ReadHeaderTimeout: 5 * time.Second,
	}
	return httpServer.ListenAndServe()
}

// Handler returns the http.Handler for testing.
func (s *Server) Handler() http.Handler {
	return s.mux
}

// recordEvent persists an event to the event store and broadcasts it via
// WebSocket. It is a no-op when EventStore is nil.
func (s *Server) recordEvent(ctx context.Context, e interfaces.Event) {
	if s.deps == nil || s.deps.EventStore == nil {
		return
	}
	event, _ := s.deps.EventStore.Append(ctx, e)
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

// decodeJSON decodes a JSON request body into v.
func decodeJSON(r *http.Request, v interface{}) error {
	defer func() { _ = r.Body.Close() }()
	return json.NewDecoder(r.Body).Decode(v)
}
