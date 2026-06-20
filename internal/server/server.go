// Package server provides the HTTP server that serves the web UI and API.
// It embeds the frontend build via go:embed and serves it in production.
// Blueprint references: Sec 3 (Architecture), Sec 25 (Phase 1).
package server

import (
	"embed"
	"io/fs"
	"log"
	"net/http"
	"strings"
)

//go:embed all:dist
var frontendFS embed.FS

// Server is the main HTTP server for the Local Agent Interface.
type Server struct {
	mux *http.ServeMux
}

// New creates a new Server with the health check and embedded frontend.
func New() *Server {
	s := &Server{mux: http.NewServeMux()}
	s.routes()
	return s
}

// routes sets up all HTTP routes.
func (s *Server) routes() {
	// Health check endpoint (Blueprint Sec 25 — basic HTTP server).
	s.mux.HandleFunc("GET /health", s.handleHealth)

	// API routes will be added by subagents (pairing, workspace, events, etc.)
	// For now, just the health check and frontend serving.

	// Serve embedded frontend in production.
	s.serveFrontend()
}

// handleHealth responds with a simple JSON health check.
func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(http.StatusOK)
	_, _ = w.Write([]byte(`{"status":"ok"}`))
}

// serveFrontend sets up the embedded React build as static files.
func (s *Server) serveFrontend() {
	// Get the dist subdirectory from the embedded FS.
	distFS, err := fs.Sub(frontendFS, "dist")
	if err != nil {
		log.Printf("WARNING: frontend dist not embedded: %v", err)
		return
	}

	fileServer := http.FileServer(http.FS(distFS))

	// Serve static assets from /assets/ directly.
	s.mux.Handle("GET /assets/", fileServer)

	// SPA fallback: any non-API route serves index.html.
	s.mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		// Skip API routes (they'll be handled by their own handlers).
		if strings.HasPrefix(r.URL.Path, "/api/") || strings.HasPrefix(r.URL.Path, "/ws") {
			http.NotFound(w, r)
			return
		}

		// Try to serve the actual file first.
		path := r.URL.Path
		if path == "/" {
			path = "/index.html"
		}

		// Check if the file exists in the embedded FS.
		if _, err := fs.Stat(distFS, strings.TrimPrefix(path, "/")); err == nil {
			fileServer.ServeHTTP(w, r)
			return
		}

		// SPA fallback: serve index.html for client-side routing.
		r.URL.Path = "/"
		fileServer.ServeHTTP(w, r)
	})
}

// ListenAndServe starts the HTTP server on the given address.
func (s *Server) ListenAndServe(addr string) error {
	log.Printf("Server listening on %s", addr)
	return http.ListenAndServe(addr, s.mux)
}

// Handler returns the http.Handler for testing.
func (s *Server) Handler() http.Handler {
	return s.mux
}
