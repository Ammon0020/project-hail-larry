// Package server provides the HTTP server that serves the web UI and API.
// Blueprint references: Sec 3 (Architecture), Sec 19 (Authentication),
// Sec 13 (Workspace), Sec 11 (Events), Sec 8 (Permissions), Sec 6 (ACP).
package server

import (
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"strconv"
	"strings"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/search"
	"github.com/adama/local-agent/internal/uploads"
	"golang.org/x/time/rate"
)

// ----------------------------------------------------------------------------
// Pairing Handlers (Blueprint Sec 19)
// ----------------------------------------------------------------------------

// Pairing rate limiting (Finding 8.1).
//
// The pairing endpoints (/api/pair/initiate, /api/pair/verify-passcode,
// /api/pair/verify-token) are unauthenticated by design — a device must be
// able to pair before it has a credential. Each initiate call mints a QR PNG
// on disk and an in-memory session, so an unauthenticated LAN attacker can
// exhaust disk/inodes by hammering the endpoint. To mitigate this we apply a
// per-IP token-bucket rate limit using golang.org/x/time/rate.
//
// pairRateLimitPerMinute is the maximum number of pairing requests allowed per
// client IP per minute. The bucket is refilled continuously, so a client may
// briefly burst up to pairRateBurst requests and then sustain
// pairRateLimitPerMinute/60 requests per second.
//
// The limiter map is package-level (the Server struct lives in server.go and
// this file owns the pairing handlers). A mutex guards concurrent access; the
// map grows lazily and is never pruned, which is acceptable for a LAN-facing
// daemon with a small attacker surface — the entries are tiny (a few dozen
// bytes each) and bounded by the number of distinct client IPs.
const (
	pairRateLimitPerMinute = 5
	pairRateBurst          = 5
)

var (
	pairLimiters   = make(map[string]*rate.Limiter)
	pairLimitersMu sync.Mutex
)

// getPairLimiter returns the per-IP rate limiter for pairing endpoints,
// creating one on first use. Each limiter allows pairRateBurst requests in an
// initial burst and then refills at pairRateLimitPerMinute requests per minute.
func getPairLimiter(ip string) *rate.Limiter {
	pairLimitersMu.Lock()
	defer pairLimitersMu.Unlock()
	lim, ok := pairLimiters[ip]
	if !ok {
		// rate.NewLimiter takes events-per-second and burst. Convert the
		// per-minute limit to per-second (5/min => 1/12s).
		lim = rate.NewLimiter(rate.Every(time.Minute/pairRateLimitPerMinute), pairRateBurst)
		pairLimiters[ip] = lim
	}
	return lim
}

// allowPairRequest reports whether the client behind r is still within the
// pairing rate limit. It extracts the client IP from RemoteAddr (the host
// portion of "host:port") and consults the per-IP limiter without blocking.
func allowPairRequest(r *http.Request) bool {
	ip, _, err := net.SplitHostPort(r.RemoteAddr)
	if err != nil {
		// RemoteAddr had no port; treat the whole string as the host.
		ip = r.RemoteAddr
	}
	return getPairLimiter(ip).Allow()
}

// handlePairInitiate creates a new pairing session with QR code and mnemonic.
func (s *Server) handlePairInitiate(w http.ResponseWriter, r *http.Request) {
	if !allowPairRequest(r) {
		writeError(w, http.StatusTooManyRequests, "pairing rate limit exceeded, try again later")
		return
	}

	var req struct {
		Host string `json:"host"`
		Port int    `json:"port"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		// An empty body is valid: fall back to the daemon's configured
		// host/port below. A malformed body is a client error and must be
		// rejected rather than silently swallowed (fail loudly), matching the
		// other pair handlers.
		if !errors.Is(err, io.EOF) {
			writeError(w, http.StatusBadRequest, "invalid request body")
			return
		}
	}

	// Populate defaults from the daemon config when the caller omitted them.
	// The daemon binds to 0.0.0.0 by default, which is not a connectable
	// address for another device, so map the wildcard to localhost using the
	// same convention the CLI uses (cmd/app pairingHost).
	if req.Host == "" {
		req.Host = localhost
		if s.deps.Config != nil && s.deps.Config.Host != "" && s.deps.Config.Host != "0.0.0.0" {
			req.Host = s.deps.Config.Host
		}
	}
	if req.Port == 0 {
		if s.deps.Config != nil && s.deps.Config.Port != 0 {
			req.Port = s.deps.Config.Port
		} else {
			req.Port = 7337
		}
	}

	session, err := s.deps.PairingMgr.CreateSession(req.Host, req.Port)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, session)
}

// handlePairVerifyPasscode verifies a mnemonic passcode and issues a device credential.
func (s *Server) handlePairVerifyPasscode(w http.ResponseWriter, r *http.Request) {
	if !allowPairRequest(r) {
		writeError(w, http.StatusTooManyRequests, "pairing rate limit exceeded, try again later")
		return
	}

	var req struct {
		Passcode   string `json:"passcode"`
		DeviceName string `json:"deviceName"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	cred, err := s.deps.PairingMgr.VerifyPasscode(req.Passcode, req.DeviceName)
	if err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, cred)
}

// handlePairVerifyToken verifies a QR token and issues a device credential.
func (s *Server) handlePairVerifyToken(w http.ResponseWriter, r *http.Request) {
	if !allowPairRequest(r) {
		writeError(w, http.StatusTooManyRequests, "pairing rate limit exceeded, try again later")
		return
	}

	var req struct {
		Token      string `json:"token"`
		DeviceName string `json:"deviceName"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	cred, err := s.deps.PairingMgr.VerifyToken(req.Token, req.DeviceName)
	if err != nil {
		writeError(w, http.StatusUnauthorized, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, cred)
}

// handleListDevices returns all paired devices.
func (s *Server) handleListDevices(w http.ResponseWriter, _ *http.Request) {
	devices := s.deps.PairingMgr.ListDevices()
	writeJSON(w, http.StatusOK, devices)
}

// handleRevokeDevice revokes a paired device's access.
func (s *Server) handleRevokeDevice(w http.ResponseWriter, r *http.Request) {
	deviceID := r.PathValue("id")
	if err := s.deps.PairingMgr.RevokeDevice(deviceID); err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{statusKey: "revoked"})
}

// ----------------------------------------------------------------------------
// Workspace Handlers (Blueprint Sec 13)
// ----------------------------------------------------------------------------

// handleListWorkspaces returns all registered workspaces.
func (s *Server) handleListWorkspaces(w http.ResponseWriter, r *http.Request) {
	workspaces, err := s.deps.WorkspaceMgr.List(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, workspaces)
}

// handleRegisterWorkspace registers a new workspace directory.
func (s *Server) handleRegisterWorkspace(w http.ResponseWriter, r *http.Request) {
	var req struct {
		Path string `json:"path"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	ws, err := s.deps.WorkspaceMgr.Register(r.Context(), req.Path)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	writeJSON(w, http.StatusCreated, ws)
}

// handleFileTree returns the file tree for a workspace.
func (s *Server) handleFileTree(w http.ResponseWriter, r *http.Request) {
	workspaceID := r.PathValue("id")
	tree, err := s.deps.WorkspaceMgr.FileTree(r.Context(), workspaceID)
	if err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, tree)
}

// handleReadFile returns the content of a file in a workspace.
func (s *Server) handleReadFile(w http.ResponseWriter, r *http.Request) {
	workspaceID := r.PathValue("id")
	relPath := r.URL.Query().Get("path")
	if relPath == "" {
		writeError(w, http.StatusBadRequest, "missing 'path' query parameter")
		return
	}

	content, revision, err := s.deps.WorkspaceMgr.ReadFile(r.Context(), workspaceID, relPath)
	if err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]interface{}{
		"content":  content,
		"revision": revision,
		"path":     relPath,
	})
}

// handleWriteFile writes content to a file in a workspace.
func (s *Server) handleWriteFile(w http.ResponseWriter, r *http.Request) {
	workspaceID := r.PathValue("id")
	var req struct {
		Path             string `json:"path"`
		Content          string `json:"content"`
		ExpectedRevision int64  `json:"expectedRevision"`
	}
	// File writes may legitimately carry large bodies, so use a higher
	// limit than the default 10 MB.
	const fileWriteMaxBodyBytes int64 = 50 << 20
	if err := decodeJSONLimit(w, r, &req, fileWriteMaxBodyBytes); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	newRevision, err := s.deps.WorkspaceMgr.WriteFile(r.Context(), workspaceID, req.Path, req.Content, req.ExpectedRevision)
	if err != nil {
		writeError(w, http.StatusConflict, err.Error())
		return
	}

	// Emit FileRevisionUpdated event.
	s.recordEvent(r.Context(), interfaces.Event{
		Type:      interfaces.EventFileRevisionUpdated,
		SessionID: "",
		Content:   req.Path,
	})

	writeJSON(w, http.StatusOK, map[string]interface{}{"revision": newRevision, "path": req.Path})
}

// handleSearch runs a workspace-wide content search (Blueprint Sec 17 — file
// search). Query params:
//
//	pattern      (required) regex to search for
//	ignoreCase   (optional) "1"/"true" for case-insensitive (default false)
//	maxResults   (optional) cap on results (default 200)
//	filePattern  (optional) glob restricting file names (e.g. "*.go")
//	contextLines (optional) context lines around each match (rg only)
//
// Returns 400 on an empty or invalid pattern, 404 on an unknown workspace,
// and 500 on other backend errors.
func (s *Server) handleSearch(w http.ResponseWriter, r *http.Request) {
	workspaceID := r.PathValue("id")
	pattern := r.URL.Query().Get("pattern")
	if strings.TrimSpace(pattern) == "" {
		writeError(w, http.StatusBadRequest, "missing 'pattern' query parameter")
		return
	}

	opts := search.Options{
		Pattern:      pattern,
		IgnoreCase:   r.URL.Query().Get("ignoreCase") == "1" || r.URL.Query().Get("ignoreCase") == "true",
		FilePattern:  r.URL.Query().Get("filePattern"),
		ContextLines: queryIntDefault(r, "contextLines", 0),
		MaxResults:   queryIntDefault(r, "maxResults", 0), // 0 => search.Search applies its default.
	}

	results, err := s.deps.WorkspaceMgr.Search(r.Context(), workspaceID, pattern, opts)
	if err != nil {
		// A bad regex surfaces from search.Search as a wrapped "invalid pattern"
		// error — map it to a 400 so the frontend can show a helpful message.
		msg := err.Error()
		if strings.Contains(msg, "invalid pattern") || strings.Contains(msg, "error parsing regexp") {
			writeError(w, http.StatusBadRequest, msg)
			return
		}
		// "workspace not found" comes from the manager lookup.
		if strings.Contains(msg, "workspace not found") {
			writeError(w, http.StatusNotFound, msg)
			return
		}
		writeError(w, http.StatusInternalServerError, msg)
		return
	}

	writeJSON(w, http.StatusOK, results)
}

// queryIntDefault parses an integer query parameter, returning the fallback
// when the parameter is absent or not a valid integer.
func queryIntDefault(r *http.Request, key string, fallback int) int {
	v := r.URL.Query().Get(key)
	if v == "" {
		return fallback
	}
	n, err := strconv.Atoi(v)
	if err != nil {
		return fallback
	}
	return n
}

// ----------------------------------------------------------------------------
// Event Handlers (Blueprint Sec 11)
// ----------------------------------------------------------------------------

// handleGetEvents returns events across all sessions.
func (s *Server) handleGetEvents(w http.ResponseWriter, r *http.Request) {
	afterID, limit := parseEventParams(r)

	events, err := s.deps.EventStore.QueryAll(r.Context(), afterID, limit)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, events)
}

// handleGetSessionEvents returns events for a specific session.
func (s *Server) handleGetSessionEvents(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("sessionId")
	afterID, limit := parseEventParams(r)

	events, err := s.deps.EventStore.Query(r.Context(), sessionID, afterID, limit)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, events)
}

// parseEventParams extracts the after cursor and limit from query params,
// applying a default limit of 1000 when unset or zero. The higher default
// avoids truncating long streaming responses (250+ events are common with
// mistral-vibe). Callers may still request fewer events via ?limit=N.
//
// The limit is capped at maxEventLimit (10000) to prevent a client from
// requesting an unbounded result set (e.g. ?limit=999999999) which would
// force the event store to materialize and the server to serialize a huge
// response. A requested limit above the cap is silently lowered to the cap
// rather than rejected, so legitimate callers that ask for "all" still get a
// large-but-bounded page.
const maxEventLimit = 10000

func parseEventParams(r *http.Request) (afterID int64, limit int) {
	afterID, _ = strconv.ParseInt(r.URL.Query().Get("after"), 10, 64)
	limit, _ = strconv.Atoi(r.URL.Query().Get("limit"))
	if limit == 0 {
		limit = 1000
	}
	if limit > maxEventLimit {
		limit = maxEventLimit
	}
	return afterID, limit
}

// ----------------------------------------------------------------------------
// Session/Agent Handlers (Blueprint Sec 6, 9, 10)
// ----------------------------------------------------------------------------

// handleListAgents returns registered agents and their models.
func (s *Server) handleListAgents(w http.ResponseWriter, r *http.Request) {
	agents, err := s.deps.ACPClient.ListAgents(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, agents)
}

// handleUpsertAgent adds or updates an agent.
func (s *Server) handleUpsertAgent(w http.ResponseWriter, r *http.Request) {
	var agent acp.AgentInfo
	if err := decodeJSON(w, r, &agent); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	if s.deps.Config != nil {
		_ = s.deps.Config.UpsertAgent(agent)
	}

	s.deps.ACPClient.RegisterAgent(agent)
	writeJSON(w, http.StatusOK, agent)
}

// handleDeleteAgent removes an agent.
func (s *Server) handleDeleteAgent(w http.ResponseWriter, r *http.Request) {
	id := r.PathValue("id")

	if s.deps.Config != nil {
		_ = s.deps.Config.DeleteAgent(id)
	}

	s.deps.ACPClient.RemoveAgent(id)
	writeJSON(w, http.StatusOK, map[string]string{statusKey: "deleted"})
}

// handleAutodetectAgents triggers manual autodetection.
func (s *Server) handleAutodetectAgents(w http.ResponseWriter, _ *http.Request) {
	detected := acp.Autodetect()
	writeJSON(w, http.StatusOK, detected)
}

// handleListSessions returns all conversations with their metadata.
func (s *Server) handleListSessions(w http.ResponseWriter, _ *http.Request) {
	sessions := s.deps.ACPClient.ListSessions()
	result := make([]interfaces.SessionInfo, 0, len(sessions))
	for _, sess := range sessions {
		info := interfaces.SessionInfo{
			ID:        sess.ID,
			Name:      sess.Name,
			Status:    sess.Status,
			AgentID:   sess.AgentID,
			ModelID:   sess.ModelID,
			Workspace: sess.Workspace,
			CreatedAt: sess.CreatedAt,
			UpdatedAt: sess.UpdatedAt,
		}
		if info.Name == "" {
			info.Name = fmt.Sprintf("Session %s", shortSessionID(sess.ID))
		}
		result = append(result, info)
	}
	writeJSON(w, http.StatusOK, result)
}

// handleGetSession returns a single conversation by ID. It delegates to the
// ACP client's GetSessionInfo so the server layer depends only on the
// interfaces.SessionInfo projection, not the concrete acp.Session type.
func (s *Server) handleGetSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")

	info, err := s.deps.ACPClient.GetSessionInfo(sessionID)
	if err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	// Match handleListSessions: fall back to a derived name when unset.
	if info.Name == "" {
		info.Name = fmt.Sprintf("Session %s", shortSessionID(info.ID))
	}

	writeJSON(w, http.StatusOK, info)
}

// handleExportSession renders a session's event history as a readable markdown
// transcript (via acp.ExportConversation, with no byte truncation so the full
// conversation is preserved) and returns it as a text/markdown attachment. The
// download filename is derived from the session name — sanitized to a safe
// filename slug — falling back to the session ID when the name is empty.
//
// The session is looked up via ACPClient.GetSessionInfo so the handler depends
// only on the interfaces.SessionInfo projection, mirroring handleGetSession. A
// missing session yields 404; an EventStore error yields 500.
func (s *Server) handleExportSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")

	// Look up the session to get its name for the download filename. A missing
	// session is a 404 — there is nothing to export.
	info, err := s.deps.ACPClient.GetSessionInfo(sessionID)
	if err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	// Render the full transcript with no truncation (maxBytes=0). The event
	// store is the source of truth; ExportConversation returns "" with a nil
	// error when the store has no events for the session, which we still serve
	// as an empty markdown file so the download always succeeds.
	markdown, err := acp.ExportConversation(r.Context(), s.deps.EventStore, sessionID, 0)
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}

	// Derive a safe filename from the session name, falling back to the session
	// ID (then a generic "session") when the name is empty. sanitizeFilename
	// strips path separators and other characters that are unsafe in download
	// filenames across operating systems.
	name := info.Name
	if name == "" {
		name = fmt.Sprintf("Session %s", shortSessionID(info.ID))
	}
	filename := sanitizeFilename(name)
	if filename == "" {
		filename = sessionID
	}
	if filename == "" {
		filename = "session"
	}
	filename += ".md"

	w.Header().Set("Content-Type", "text/markdown; charset=utf-8")
	w.Header().Set("Content-Disposition", fmt.Sprintf(`attachment; filename="%s"`, filename))
	w.WriteHeader(http.StatusOK)
	_, _ = io.WriteString(w, markdown)
}

// sanitizeFilename collapses a session name into a safe filename slug. It
// keeps alphanumerics, dashes, and underscores, and replaces every other rune
// (including path separators, control characters, and whitespace) with an
// underscore. An all-unsafe input yields an empty string, which the caller is
// expected to fall back from. The result is not length-capped; session names
// are user-controlled but short, and overly long names are still valid
// filenames on every supported platform.
func sanitizeFilename(name string) string {
	var b strings.Builder
	for _, c := range name {
		switch {
		case c >= 'a' && c <= 'z', c >= 'A' && c <= 'Z', c >= '0' && c <= '9', c == '-', c == '_':
			b.WriteRune(c)
		default:
			b.WriteByte('_')
		}
	}
	return strings.Trim(b.String(), "_")
}

// handlePatchSession renames a conversation and/or rebinds it to a different
// agent/model. Body: { "name"?: string, "agentId"?: string, "modelId"?: string, "maxTransferBytes"?: int }.
func (s *Server) handlePatchSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	var req struct {
		Name             *string `json:"name"`
		AgentID          *string `json:"agentId"`
		ModelID          *string `json:"modelId"`
		MaxTransferBytes *int    `json:"maxTransferBytes"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	if req.Name != nil {
		if err := s.deps.ACPClient.RenameSession(sessionID, *req.Name); err != nil {
			writeError(w, http.StatusNotFound, err.Error())
			return
		}
	}

	// Model-only change: switch the model on the live session without restart.
	// This preserves the full conversation context (ACP session/set_config_option).
	// Falls back to RebindSession internally when the agent doesn't advertise a
	// model config option.
	if req.AgentID == nil && req.ModelID != nil {
		if err := s.deps.ACPClient.SwitchModel(r.Context(), sessionID, *req.ModelID); err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
		writeJSON(w, http.StatusOK, map[string]string{statusKey: statusUpdated})
		return
	}

	// Full rebind (agent + model): requires both to be specified.
	if req.AgentID != nil && req.ModelID != nil {
		maxTransfer := 0
		if req.MaxTransferBytes != nil {
			maxTransfer = *req.MaxTransferBytes
		}
		if _, err := s.deps.ACPClient.RebindSession(r.Context(), sessionID, *req.AgentID, *req.ModelID, maxTransfer); err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
	}

	writeJSON(w, http.StatusOK, map[string]string{statusKey: statusUpdated})
}

// handleCreateSession creates a new agent session.
func (s *Server) handleCreateSession(w http.ResponseWriter, r *http.Request) {
	var req struct {
		AgentID     string `json:"agentId"`
		ModelID     string `json:"modelId"`
		WorkspaceID string `json:"workspaceId"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	session, err := s.deps.ACPClient.CreateSession(r.Context(), req.AgentID, req.ModelID, req.WorkspaceID)
	if err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}
	writeJSON(w, http.StatusCreated, session)
}

// handleSendPrompt sends a prompt to an agent session.
func (s *Server) handleSendPrompt(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	var req struct {
		Content     string `json:"content"`
		Attachments []struct {
			ID       string `json:"id"`
			Name     string `json:"name"`
			MimeType string `json:"mimeType"`
		} `json:"attachments"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	content := strings.TrimSpace(req.Content)
	if content == "" {
		writeError(w, http.StatusBadRequest, "prompt content is required")
		return
	}

	// Resolve each attachment ID to an on-disk path via the uploads manager so
	// the ACP transport can read the file and build inline image / resource
	// link blocks. An unresolvable ID means the frontend sent a stale or
	// invalid reference — reject the whole request rather than sending a
	// partial prompt.
	var attachments []interfaces.Attachment
	if len(req.Attachments) > 0 {
		if s.deps.Uploads == nil {
			writeError(w, http.StatusBadRequest, "uploads not configured")
			return
		}
		attachments = make([]interfaces.Attachment, 0, len(req.Attachments))
		for _, att := range req.Attachments {
			absPath, err := s.deps.Uploads.Get(sessionID, att.ID)
			if err != nil {
				writeError(w, http.StatusBadRequest, fmt.Sprintf("attachment %s not found", att.ID))
				return
			}
			attachments = append(attachments, interfaces.Attachment{
				ID:       att.ID,
				Name:     att.Name,
				MimeType: att.MimeType,
				Path:     absPath,
				URI:      "file://" + absPath,
			})
		}
	}

	if err := s.deps.ACPClient.SendPrompt(r.Context(), sessionID, content, attachments); err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]string{statusKey: "sent"})
}

// handleCancelSession cancels a running session.
func (s *Server) handleCancelSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")

	if err := s.deps.ACPClient.CancelSession(r.Context(), sessionID); err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	s.recordEvent(r.Context(), interfaces.Event{
		Type:      interfaces.EventSessionCancelled,
		SessionID: sessionID,
	})

	writeJSON(w, http.StatusOK, map[string]string{statusKey: "cancelled"})
}

// handleCloseSession closes a session.
func (s *Server) handleCloseSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")

	if err := s.deps.ACPClient.CloseSession(r.Context(), sessionID); err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	// Best-effort cleanup of per-session uploads now that the session is
	// closed. The ACP client is intentionally decoupled from the uploads
	// store, so the cleanup hook lives here in the server layer.
	if s.deps.Uploads != nil {
		_ = s.deps.Uploads.RemoveSession(sessionID)
	}

	writeJSON(w, http.StatusOK, map[string]string{statusKey: "closed"})
}

// handleUpload accepts a multipart file upload for a session, validates it via
// the uploads manager (magic-byte detection, size cap), and responds with the
// upload metadata including a URL the frontend can use for <img src>.
func (s *Server) handleUpload(w http.ResponseWriter, r *http.Request) {
	if s.deps == nil || s.deps.Uploads == nil {
		writeError(w, http.StatusServiceUnavailable, "uploads not configured")
		return
	}
	sessionID := r.PathValue("id")
	if !isValidSessionID(sessionID) {
		writeError(w, http.StatusBadRequest, "invalid session id")
		return
	}

	// Cap the request body so an oversized upload can't exhaust server memory.
	// +1KB accounts for multipart headers/boundaries on top of the file payload.
	r.Body = http.MaxBytesReader(w, r.Body, uploads.MaxUploadBytes+1024)

	// MaxUploadBytes (10 MB) matches the uploads manager's internal cap.
	if err := r.ParseMultipartForm(uploads.MaxUploadBytes); err != nil {
		writeError(w, http.StatusBadRequest, "invalid multipart form")
		return
	}
	defer func() {
		if r.MultipartForm != nil {
			_ = r.MultipartForm.RemoveAll()
		}
	}()

	file, header, err := r.FormFile("file")
	if err != nil {
		writeError(w, http.StatusBadRequest, "missing 'file' field in multipart form")
		return
	}
	defer func() { _ = file.Close() }()

	stored, err := s.deps.Uploads.Store(sessionID, header.Filename, file)
	if err != nil {
		writeError(w, http.StatusBadRequest, "failed to store upload")
		return
	}

	writeJSON(w, http.StatusCreated, map[string]any{
		"id":       stored.ID,
		"name":     stored.Name,
		"mimeType": stored.MimeType,
		"url":      fmt.Sprintf("/api/sessions/%s/uploads/%s", sessionID, stored.ID),
		"size":     stored.Size,
	})
}

// handleServeUpload serves a previously stored upload file for a session. The
// uploads manager resolves the upload ID to an on-disk path; http.ServeFile
// sets the Content-Type from the stored extension (chosen by magic-byte
// detection during Store).
func (s *Server) handleServeUpload(w http.ResponseWriter, r *http.Request) {
	if s.deps == nil || s.deps.Uploads == nil {
		writeError(w, http.StatusServiceUnavailable, "uploads not configured")
		return
	}
	sessionID := r.PathValue("id")
	if !isValidSessionID(sessionID) {
		writeError(w, http.StatusBadRequest, "invalid session id")
		return
	}
	uploadID := r.PathValue("uploadID")

	path, err := s.deps.Uploads.Get(sessionID, uploadID)
	if err != nil {
		writeError(w, http.StatusNotFound, "upload not found")
		return
	}

	http.ServeFile(w, r, path)
}

// handleSessionContext accepts frontend-reported editor state (currently open
// files, recently edited files, and the current editor selection) for a session
// and updates the OpenFilesTracker so the prompt middleware pipeline can inject
// it. The session ID is accepted for future per-session tracking but the current
// tracker is process-global. Body:
//
//	{
//	  "openFiles": ["path1", ...],
//	  "recentEdits": ["path3", ...],
//	  "selection": { "path": "rel/path", "startLine": 1, "endLine": 3, "text": "..." }
//	}
//
// All fields are optional; omitted fields leave the tracker unchanged. A
// selection with empty text clears any previously stored selection.
func (s *Server) handleSessionContext(w http.ResponseWriter, r *http.Request) {
	if s.deps == nil || s.deps.OpenFilesTracker == nil {
		writeError(w, http.StatusServiceUnavailable, "open-files tracking not configured")
		return
	}
	var req struct {
		OpenFiles   []string `json:"openFiles"`
		RecentEdits []string `json:"recentEdits"`
		Selection   *struct {
			Path      string `json:"path"`
			StartLine int    `json:"startLine"`
			EndLine   int    `json:"endLine"`
			Text      string `json:"text"`
		} `json:"selection"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}
	if req.OpenFiles != nil {
		s.deps.OpenFilesTracker.SetOpenFiles(req.OpenFiles)
	}
	if req.RecentEdits != nil {
		s.deps.OpenFilesTracker.SetRecentEdits(req.RecentEdits)
	}
	if req.Selection != nil {
		s.deps.OpenFilesTracker.SetSelection(acp.EditorSelection{
			Path:      req.Selection.Path,
			StartLine: req.Selection.StartLine,
			EndLine:   req.Selection.EndLine,
			Text:      req.Selection.Text,
		})
	}
	writeJSON(w, http.StatusOK, map[string]string{statusKey: statusUpdated})
}

// ----------------------------------------------------------------------------
// Permission Handlers (Blueprint Sec 8)
// ----------------------------------------------------------------------------

// handlePendingPermissions returns all pending permission requests.
func (s *Server) handlePendingPermissions(w http.ResponseWriter, _ *http.Request) {
	pending := s.deps.PermissionMgr.GetPending()
	writeJSON(w, http.StatusOK, pending)
}

// handleRespondPermission responds to a permission request.
func (s *Server) handleRespondPermission(w http.ResponseWriter, r *http.Request) {
	requestID := r.PathValue("id")
	var req struct {
		Decision string `json:"decision"`
	}
	if err := decodeJSON(w, r, &req); err != nil {
		writeError(w, http.StatusBadRequest, "invalid request body")
		return
	}

	// Capture the request's session id and offered options before responding —
	// the pending request is removed once Respond resolves it.
	sessionID := ""
	var offered []interfaces.PermissionOptionInfo
	for _, p := range s.deps.PermissionMgr.GetPending() {
		if p.ID == requestID {
			sessionID = p.SessionID
			offered = p.OptionDetails
			break
		}
	}

	decision := interfaces.PermissionDecision(req.Decision)
	if err := s.deps.PermissionMgr.Respond(r.Context(), requestID, decision); err != nil {
		writeError(w, http.StatusBadRequest, err.Error())
		return
	}

	// Classify the chosen option as grant or deny for the event log. ACP reject
	// kinds (reject_once/reject_always) and our "deny" decision count as denials.
	eventType := interfaces.EventPermissionGranted
	if isDenyDecision(req.Decision, offered) {
		eventType = interfaces.EventPermissionDenied
	}
	s.recordEvent(r.Context(), interfaces.Event{
		Type:      eventType,
		SessionID: sessionID,
		RequestID: requestID,
	})

	writeJSON(w, http.StatusOK, map[string]string{statusKey: "responded"})
}

// isDenyDecision reports whether the chosen decision represents a denial, using
// the offered option kinds when available and falling back to the decision text.
func isDenyDecision(decision string, offered []interfaces.PermissionOptionInfo) bool {
	for _, o := range offered {
		if o.ID == decision {
			return strings.HasPrefix(o.Kind, "reject") || o.Kind == string(interfaces.PermissionDeny)
		}
	}
	return decision == string(interfaces.PermissionDeny) || strings.HasPrefix(decision, "reject")
}

// shortSessionID returns the first 8 characters of a session ID, or the full
// ID if it is shorter than 8 characters. This guards against a slice-bounds
// panic when deriving display names like "Session <shortID>" — the ACPClient
// contract does not guarantee a minimum ID length, so an empty or short ID
// (e.g. from a stub or future ID scheme) must not crash the handler.
func shortSessionID(id string) string {
	if len(id) <= 8 {
		return id
	}
	return id[:8]
}

// isValidSessionID validates a session ID taken from a URL path parameter
// before it is used as a filesystem path component (uploads root) or passed to
// the ACP client. Session IDs are backend-generated opaque tokens shaped like
// "sess-" + 16 hex chars (see internal/acp.generateSessionID), so we reject
// empty strings, path separators, and ".." segments rather than requiring a
// strict hex shape. This rejects path-traversal payloads like "../../foo"
// early, before they can escape the uploads root or be passed to os.RemoveAll.
func isValidSessionID(id string) bool {
	if id == "" || id == "." || id == ".." {
		return false
	}
	for _, c := range id {
		if c == '/' || c == '\\' {
			return false
		}
		if c < 0x20 {
			return false
		}
	}
	return !strings.Contains(id, "..")
}
