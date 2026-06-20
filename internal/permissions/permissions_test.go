package permissions

import (
	"context"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// TestRequestAndRespond verifies the basic request-respond flow.
func TestRequestAndRespond(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "session-1",
		Tool:      "shell",
		Command:   "npm test",
	}

	// Start the request in a goroutine — it blocks until responded.
	resultCh := make(chan interfaces.PermissionDecision, 1)
	errCh := make(chan error, 1)
	go func() {
		decision, err := m.Request(context.Background(), req)
		if err != nil {
			errCh <- err
			return
		}
		resultCh <- decision
	}()

	// Give the goroutine time to register the request.
	time.Sleep(50 * time.Millisecond)

	// Get the pending request to find its ID.
	pending := m.GetPending()
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending request, got %d", len(pending))
	}

	// Respond with allow_once.
	if err := m.Respond(context.Background(), pending[0].ID, interfaces.PermissionAllowOnce); err != nil {
		t.Fatalf("respond: %v", err)
	}

	// Verify the decision.
	select {
	case decision := <-resultCh:
		if decision != interfaces.PermissionAllowOnce {
			t.Errorf("expected allow_once, got %s", decision)
		}
	case err := <-errCh:
		t.Fatalf("request error: %v", err)
	case <-time.After(time.Second):
		t.Fatal("timed out waiting for decision")
	}
}

// TestRequestTimeout verifies that a request times out when no response is given.
func TestRequestTimeout(t *testing.T) {
	m := NewManager()

	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	req := interfaces.PermissionRequest{
		SessionID: "session-1",
		Tool:      "shell",
		Command:   "rm -rf /",
	}

	decision, err := m.Request(ctx, req)
	if err == nil {
		t.Error("expected timeout error")
	}
	if decision != interfaces.PermissionDeny {
		t.Errorf("expected deny on timeout, got %s", decision)
	}
}

// TestRespondNotFound verifies that responding to a nonexistent request fails.
func TestRespondNotFound(t *testing.T) {
	m := NewManager()

	err := m.Respond(context.Background(), "nonexistent", interfaces.PermissionAllowOnce)
	if err == nil {
		t.Error("expected error for nonexistent request")
	}
}

// TestFirstResponseWins verifies that the first response is accepted and subsequent ones rejected.
func TestFirstResponseWins(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "session-1",
		Tool:      "edit_file",
		Target:    "server.js",
	}

	resultCh := make(chan interfaces.PermissionDecision, 1)
	go func() {
		decision, _ := m.Request(context.Background(), req)
		resultCh <- decision
	}()

	time.Sleep(50 * time.Millisecond)

	pending := m.GetPending()
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending, got %d", len(pending))
	}

	// First response should succeed.
	err := m.Respond(context.Background(), pending[0].ID, interfaces.PermissionAllowSession)
	if err != nil {
		t.Fatalf("first respond: %v", err)
	}

	// Second response should fail (request already resolved and cleaned up).
	time.Sleep(50 * time.Millisecond)
	err = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
	if err == nil {
		t.Error("expected error for second response")
	}

	// Verify the first decision was used.
	decision := <-resultCh
	if decision != interfaces.PermissionAllowSession {
		t.Errorf("expected allow_session, got %s", decision)
	}
}

// TestAuditLog verifies that decisions are recorded in the audit log.
func TestAuditLog(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "session-1",
		Tool:      "shell",
		Command:   "npm run build",
	}

	go func() {
		m.Request(context.Background(), req)
	}()

	time.Sleep(50 * time.Millisecond)

	pending := m.GetPending()
	m.Respond(context.Background(), pending[0].ID, interfaces.PermissionAllowAlways)

	time.Sleep(50 * time.Millisecond)

	log := m.GetAuditLog()
	if len(log) != 1 {
		t.Fatalf("expected 1 audit entry, got %d", len(log))
	}

	if log[0].Tool != "shell" {
		t.Errorf("expected tool 'shell', got %s", log[0].Tool)
	}
	if log[0].Decision != string(interfaces.PermissionAllowAlways) {
		t.Errorf("expected decision 'allow_always', got %s", log[0].Decision)
	}
}

// TestInvalidDecision verifies that an invalid decision is rejected.
func TestInvalidDecision(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "session-1",
		Tool:      "shell",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowOnce, interfaces.PermissionDeny},
	}

	go func() {
		m.Request(context.Background(), req)
	}()

	time.Sleep(50 * time.Millisecond)

	pending := m.GetPending()
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending, got %d", len(pending))
	}

	// Try an option not in the allowed list.
	err := m.Respond(context.Background(), pending[0].ID, interfaces.PermissionAllowAlways)
	if err == nil {
		t.Error("expected error for invalid decision")
	}
}
