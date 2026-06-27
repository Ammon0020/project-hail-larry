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

// policyKey identifies a permission policy entry by session, tool, and a
// discriminator that scopes the cached decision. The toolKind field is keyed
// on the request's Tool title (the human-readable tool name) rather than the
// ACP "tool kind" to keep the policy granular enough to distinguish e.g.
// "edit_file" from "execute" while remaining stable across requests.
//
// target is the first affected location path for file-oriented tools, or "" if
// the request has no target. command is the raw command text and is only used
// as the discriminator for shell/execute tools, which have no location
// (target == ""). Without command in the key, a single allow_always for one
// shell command would auto-approve every subsequent shell command in the
// session regardless of content — a permission bypass. See
// policyKeyFor for the exact selection logic.
type policyKey struct {
	sessionID string
	toolKind  string
	target    string
	command   string
}

// policyKeyFor builds the cache key for a permission request.
//
// For file-oriented tools (target != "") the target path is the discriminator,
// so allow_always/allow_session auto-approve repeated operations on the same
// file. For shell/execute tools (target == "") the command text is the
// discriminator, so an allow_always for "go test" does not auto-approve
// "rm -rf /". This closes the shell-command permission bypass where every
// shell command in a session shared the empty-target key.
func policyKeyFor(req interfaces.PermissionRequest) policyKey {
	key := policyKey{sessionID: req.SessionID, toolKind: req.Tool, target: req.Target}
	if req.Target == "" {
		key.command = req.Command
	}
	return key
}

// Manager implements interfaces.PermissionManager.
type Manager struct {
	mu       sync.Mutex
	pending  map[string]*pendingRequest
	policy   map[policyKey]interfaces.PermissionDecision
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
		policy:   make(map[policyKey]interfaces.PermissionDecision),
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

	// Check the policy map before blocking. A prior allow_always or
	// allow_session decision for this (session, tool, discriminator)
	// combination auto-resolves the request immediately — no callback, no
	// blocking. The discriminator is the target path for file tools and the
	// command text for shell tools (see policyKeyFor), so a cached shell
	// decision only auto-approves the exact same command.
	//
	// Note: allow_once and bare deny are NOT auto-resolved. The codebase has
	// no reject_always constant, so reject-always auto-deny is intentionally
	// skipped (see docs/plans/execution-plan.md Work Stream 2).
	key := policyKeyFor(req)
	m.mu.Lock()
	cached, ok := m.policy[key]
	m.mu.Unlock()
	if ok && (cached == interfaces.PermissionAllowAlways || cached == interfaces.PermissionAllowSession) {
		m.recordAudit(req, cached)
		return cached, nil
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
		// Persist durable allow decisions so subsequent requests for the same
		// (session, tool, discriminator) auto-resolve without blocking the user.
		if decision == interfaces.PermissionAllowAlways || decision == interfaces.PermissionAllowSession {
			m.mu.Lock()
			m.policy[key] = decision
			m.mu.Unlock()
		}
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

// ClearSession drops all cached permission policies for the given session and
// denies any pending permission requests for it. It should be called when a
// session closes so that allow_always/allow_session decisions do not leak
// across session lifetimes and in-flight Request calls return promptly instead
// of hanging until their context deadline. The blocked Request receives a deny
// decision; its defer cleanup tolerates the pending entry already being deleted.
func (m *Manager) ClearSession(sessionID string) {
	m.mu.Lock()
	defer m.mu.Unlock()
	for k := range m.policy {
		if k.sessionID == sessionID {
			delete(m.policy, k)
		}
	}
	// Deny pending requests for this session so the agent's RequestPermission
	// RPC does not hang with no response. The send is non-blocking (the
	// response channel is buffered with capacity 1); a full channel means the
	// request was already resolved independently.
	for id, p := range m.pending {
		if p.request.SessionID == sessionID {
			select {
			case p.response <- interfaces.PermissionDeny:
			default:
			}
			delete(m.pending, id)
		}
	}
}

// maxAuditEntries bounds the in-memory audit log so a long-running daemon
// does not grow it without limit. Only the most recent entries are retained;
// older entries are evicted once the cap is exceeded.
const maxAuditEntries = 10000

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
	// Bound the in-memory audit log to the last maxAuditEntries entries.
	if len(m.auditLog) > maxAuditEntries {
		m.auditLog = m.auditLog[len(m.auditLog)-maxAuditEntries:]
	}
}

// generateID generates a cryptographically random hex string.
func generateID(byteLen int) (string, error) {
	b := make([]byte, byteLen)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}
