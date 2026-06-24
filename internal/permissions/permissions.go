// Package permissions implements the permission manager.
// Blueprint references: Sec 8 (Permission Manager).
//
// Permission requests from agents are broadcast to all paired devices.
// The first response wins. Decisions: allow-once, allow-session, allow-always, deny.
// An audit log persists all decisions.
package permissions

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"fmt"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// Manager implements interfaces.PermissionManager.
type Manager struct {
	mu       sync.Mutex
	pending  map[string]*pendingRequest
	auditLog []AuditEntry
	onReq    func(interfaces.PermissionRequest)
}

// AuditEntry records a permission decision for compliance and debugging.
type AuditEntry struct {
	RequestID string    `json:"requestId"`
	SessionID string    `json:"sessionId"`
	Tool      string    `json:"tool"`
	Command   string    `json:"command,omitempty"`
	Decision  string    `json:"decision"`
	Timestamp time.Time `json:"timestamp"`
}

// pendingRequest tracks an outstanding permission prompt.
type pendingRequest struct {
	request  interfaces.PermissionRequest
	response chan interfaces.PermissionDecision
}

// NewManager creates a new permission Manager.
func NewManager() *Manager {
	return &Manager{
		pending:  make(map[string]*pendingRequest),
		auditLog: make([]AuditEntry, 0),
	}
}

// SetCallback registers a function invoked whenever a new permission request is
// created. The server uses this to emit a PermissionRequested event and
// broadcast it to connected devices. Must be called before Request.
func (m *Manager) SetCallback(fn func(interfaces.PermissionRequest)) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.onReq = fn
}

// Request broadcasts a permission prompt and blocks until a decision is received
// or the context is cancelled. The first response wins.
func (m *Manager) Request(ctx context.Context, req interfaces.PermissionRequest) (interfaces.PermissionDecision, error) {
	if req.ID == "" {
		id, err := generateID(16)
		if err != nil {
			return "", fmt.Errorf("generate request id: %w", err)
		}
		req.ID = id
	}

	// Set default options if none provided.
	if len(req.Options) == 0 {
		req.Options = []interfaces.PermissionDecision{
			interfaces.PermissionAllowOnce,
			interfaces.PermissionAllowSession,
			interfaces.PermissionAllowAlways,
			interfaces.PermissionDeny,
		}
	}

	respCh := make(chan interfaces.PermissionDecision, 1)

	m.mu.Lock()
	m.pending[req.ID] = &pendingRequest{
		request:  req,
		response: respCh,
	}
	cb := m.onReq
	m.mu.Unlock()

	// Notify listeners (server) so the UI is prompted. Done outside the lock.
	if cb != nil {
		cb(req)
	}

	// Clean up on exit.
	defer func() {
		m.mu.Lock()
		delete(m.pending, req.ID)
		m.mu.Unlock()
	}()

	// Wait for a response or context cancellation.
	select {
	case decision := <-respCh:
		m.recordAudit(req, decision)
		return decision, nil
	case <-ctx.Done():
		return interfaces.PermissionDeny, fmt.Errorf("permission request timed out: %w", ctx.Err())
	}
}

// Respond records a decision from a device. First response wins.
// Returns an error if the request doesn't exist or already has a response.
func (m *Manager) Respond(_ context.Context, requestID string, decision interfaces.PermissionDecision) error {
	m.mu.Lock()
	pending, ok := m.pending[requestID]
	m.mu.Unlock()

	if !ok {
		return fmt.Errorf("permission request not found or already resolved: %s", requestID)
	}

	// Validate the decision is one of the allowed options.
	valid := false
	for _, opt := range pending.request.Options {
		if opt == decision {
			valid = true
			break
		}
	}
	if !valid {
		return fmt.Errorf("invalid decision %s for request %s", decision, requestID)
	}

	// Send the response (non-blocking, first call wins).
	select {
	case pending.response <- decision:
		return nil
	default:
		return fmt.Errorf("request already resolved: %s", requestID)
	}
}

// GetPending returns all pending permission requests (for re-presentation on reconnect).
func (m *Manager) GetPending() []interfaces.PermissionRequest {
	m.mu.Lock()
	defer m.mu.Unlock()

	requests := make([]interfaces.PermissionRequest, 0, len(m.pending))
	for _, p := range m.pending {
		requests = append(requests, p.request)
	}
	return requests
}

// GetAuditLog returns the audit log of all permission decisions.
func (m *Manager) GetAuditLog() []AuditEntry {
	m.mu.Lock()
	defer m.mu.Unlock()

	log := make([]AuditEntry, len(m.auditLog))
	copy(log, m.auditLog)
	return log
}

// recordAudit adds a decision to the audit log.
func (m *Manager) recordAudit(req interfaces.PermissionRequest, decision interfaces.PermissionDecision) {
	m.mu.Lock()
	defer m.mu.Unlock()

	m.auditLog = append(m.auditLog, AuditEntry{
		RequestID: req.ID,
		SessionID: req.SessionID,
		Tool:      req.Tool,
		Command:   req.Command,
		Decision:  string(decision),
		Timestamp: time.Now().UTC(),
	})
}

// generateID generates a cryptographically random hex string.
func generateID(byteLen int) (string, error) {
	b := make([]byte, byteLen)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
