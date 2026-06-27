// Package server provides the HTTP server that serves the web UI and API.
// Blueprint references: Sec 3 (Architecture), Sec 19 (Authentication),
// Sec 13 (Workspace), Sec 11 (Events), Sec 8 (Permissions), Sec 6 (ACP).
package server

import (
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"

	"github.com/adama/local-agent/internal/acp"
	"github.com/adama/local-agent/internal/interfaces"
)

// ----------------------------------------------------------------------------
// Pairing Handlers (Blueprint Sec 19)
// ----------------------------------------------------------------------------

// handlePairInitiate creates a new pairing session with QR code and mnemonic.
func (s *Server) handlePairInitiate(w http.ResponseWriter, r *http.Request) {
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
		req.Host = "localhost"
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
	writeJSON(w, http.StatusOK, map[string]string{"status": "revoked"})
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
// mistral-vibe). Callers may still request fewer events via ?limit=N, and
// there is no upper cap so arbitrarily large responses can be fetched.
func parseEventParams(r *http.Request) (afterID int64, limit int) {
	afterID, _ = strconv.ParseInt(r.URL.Query().Get("after"), 10, 64)
	limit, _ = strconv.Atoi(r.URL.Query().Get("limit"))
	if limit == 0 {
		limit = 1000
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
	writeJSON(w, http.StatusOK, map[string]string{"status": "deleted"})
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

// handlePatchSession renames a conversation and/or rebinds it to a different
// agent/model. Body: { "name"?: string, "agentId"?: string, "modelId"?: string }.
func (s *Server) handlePatchSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")
	var req struct {
		Name    *string `json:"name"`
		AgentID *string `json:"agentId"`
		ModelID *string `json:"modelId"`
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

	// A rebind requires both agent and model to be specified.
	if req.AgentID != nil && req.ModelID != nil {
		if _, err := s.deps.ACPClient.RebindSession(r.Context(), sessionID, *req.AgentID, *req.ModelID); err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
	}

	writeJSON(w, http.StatusOK, map[string]string{"status": "updated"})
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
		Content string `json:"content"`
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

	if err := s.deps.ACPClient.SendPrompt(r.Context(), sessionID, content); err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]string{"status": "sent"})
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

	writeJSON(w, http.StatusOK, map[string]string{"status": "cancelled"})
}

// handleCloseSession closes a session.
func (s *Server) handleCloseSession(w http.ResponseWriter, r *http.Request) {
	sessionID := r.PathValue("id")

	if err := s.deps.ACPClient.CloseSession(r.Context(), sessionID); err != nil {
		writeError(w, http.StatusNotFound, err.Error())
		return
	}

	writeJSON(w, http.StatusOK, map[string]string{"status": "closed"})
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

	writeJSON(w, http.StatusOK, map[string]string{"status": "responded"})
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
