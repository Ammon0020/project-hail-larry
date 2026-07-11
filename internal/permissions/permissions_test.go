package permissions

import (
	"context"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// TestRequestInvokesCallback verifies that Request notifies the registered
// callback so the server can broadcast a PermissionRequested event. This is the
// path that previously left the UI without any permission prompt.
func TestRequestInvokesCallback(t *testing.T) {
	m := NewManager()

	got := make(chan interfaces.PermissionRequest, 1)
	m.SetCallback(func(req interfaces.PermissionRequest) {
		got <- req
	})

	go func() {
		_, _ = m.Request(context.Background(), interfaces.PermissionRequest{
			SessionID: "sess-1",
			Tool:      "execute",
			Command:   "go test ./...",
			Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowOnce, interfaces.PermissionDeny},
		})
	}()

	select {
	case req := <-got:
		if req.ID == "" {
			t.Error("expected callback request to have a generated ID")
		}
		if req.Tool != "execute" {
			t.Errorf("expected tool 'execute', got %q", req.Tool)
		}
	case <-time.After(time.Second):
		t.Fatal("callback was not invoked")
	}

	// Resolve so the Request goroutine exits cleanly.
	pending := m.GetPending()
	if len(pending) == 1 {
		_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionAllowOnce)
	}
}

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

// resolveFirstRequest starts a Request in a goroutine, waits for it to register
// as pending, and responds with the given decision. It returns the decision the
// Request call returns (or fails the test on timeout). This is the shared helper
// for seeding the policy map via the blocking path.
func resolveFirstRequest(t *testing.T, m *Manager, req interfaces.PermissionRequest, decision interfaces.PermissionDecision) interfaces.PermissionDecision {
	t.Helper()

	resultCh := make(chan interfaces.PermissionDecision, 1)
	errCh := make(chan error, 1)
	go func() {
		d, err := m.Request(context.Background(), req)
		if err != nil {
			errCh <- err
			return
		}
		resultCh <- d
	}()

	// Wait for the request to register as pending.
	pending := waitForPending(t, m, 1)
	if err := m.Respond(context.Background(), pending[0].ID, decision); err != nil {
		t.Fatalf("respond: %v", err)
	}

	select {
	case d := <-resultCh:
		return d
	case err := <-errCh:
		t.Fatalf("request error: %v", err)
	case <-time.After(2 * time.Second):
		t.Fatal("timed out waiting for decision")
	}
	return ""
}

// waitForPending polls GetPending until it observes the expected number of
// pending requests (or times out). The policy auto-resolve path never creates a
// pending entry, so this is how tests assert that a request actually blocked.
func waitForPending(t *testing.T, m *Manager, want int) []interfaces.PermissionRequest {
	t.Helper()
	deadline := time.Now().Add(2 * time.Second)
	for {
		pending := m.GetPending()
		if len(pending) == want {
			return pending
		}
		if time.Now().After(deadline) {
			t.Fatalf("expected %d pending request(s), got %d", want, len(pending))
		}
		time.Sleep(10 * time.Millisecond)
	}
}

// TestPolicyAllowAlwaysAutoResolves verifies that an allow_always decision for a
// (session, tool, target) combination auto-resolves a subsequent identical
// request without blocking or invoking the callback.
func TestPolicyAllowAlwaysAutoResolves(t *testing.T) {
	m := NewManager()

	// Track callback invocations — the second request must NOT trigger it.
	callbackCount := 0
	m.SetCallback(func(_ interfaces.PermissionRequest) {
		callbackCount++
	})

	req := interfaces.PermissionRequest{
		SessionID: "sess-policy-always",
		Tool:      "edit_file",
		Target:    "main.go",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowAlways, interfaces.PermissionDeny},
	}

	// First request blocks and is resolved with allow_always.
	if d := resolveFirstRequest(t, m, req, interfaces.PermissionAllowAlways); d != interfaces.PermissionAllowAlways {
		t.Fatalf("first request: expected allow_always, got %s", d)
	}
	if callbackCount != 1 {
		t.Fatalf("expected callback invoked once after first request, got %d", callbackCount)
	}

	// Second identical request must auto-resolve immediately (no blocking).
	done := make(chan interfaces.PermissionDecision, 1)
	go func() {
		d, err := m.Request(context.Background(), req)
		if err != nil {
			t.Errorf("second request error: %v", err)
			done <- ""
			return
		}
		done <- d
	}()

	select {
	case d := <-done:
		if d != interfaces.PermissionAllowAlways {
			t.Errorf("expected auto-resolved allow_always, got %s", d)
		}
	case <-time.After(time.Second):
		t.Fatal("second request did not auto-resolve (blocked)")
	}

	if callbackCount != 1 {
		t.Errorf("expected callback still invoked once (not for auto-resolve), got %d", callbackCount)
	}
}

// TestPolicyAllowSessionAutoResolves verifies that an allow_session decision
// auto-resolves subsequent same-session requests.
func TestPolicyAllowSessionAutoResolves(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "sess-policy-session",
		Tool:      "execute",
		Command:   "go test",
		Target:    "",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowSession, interfaces.PermissionDeny},
	}

	if d := resolveFirstRequest(t, m, req, interfaces.PermissionAllowSession); d != interfaces.PermissionAllowSession {
		t.Fatalf("first request: expected allow_session, got %s", d)
	}

	// Second request auto-resolves.
	done := make(chan interfaces.PermissionDecision, 1)
	go func() {
		d, _ := m.Request(context.Background(), req)
		done <- d
	}()

	select {
	case d := <-done:
		if d != interfaces.PermissionAllowSession {
			t.Errorf("expected auto-resolved allow_session, got %s", d)
		}
	case <-time.After(time.Second):
		t.Fatal("second request did not auto-resolve (blocked)")
	}
}

// TestPolicyAllowOnceDoesNotAutoResolve verifies that an allow_once decision does
// NOT seed the policy map — a second identical request still blocks for user
// input.
func TestPolicyAllowOnceDoesNotAutoResolve(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "sess-policy-once",
		Tool:      "shell",
		Command:   "ls",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowOnce, interfaces.PermissionDeny},
	}

	if d := resolveFirstRequest(t, m, req, interfaces.PermissionAllowOnce); d != interfaces.PermissionAllowOnce {
		t.Fatalf("first request: expected allow_once, got %s", d)
	}

	// Second request must block (no auto-resolve). Use a short-timeout context
	// to verify it does not return immediately.
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()

	start := time.Now()
	_, err := m.Request(ctx, req)
	elapsed := time.Since(start)

	// It should have blocked until the context expired, not returned instantly.
	if err == nil {
		t.Fatal("expected second allow_once request to block and time out, but it returned without error")
	}
	if elapsed < 80*time.Millisecond {
		t.Errorf("expected request to block ~100ms before timeout, returned after %v", elapsed)
	}

	// Clean up the pending request so the goroutine exits.
	pending := m.GetPending()
	if len(pending) == 1 {
		_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
	}
}

// TestPolicySessionScoped verifies that a policy decision in session A does not
// affect session B. Table-driven over the two durable decision kinds.
func TestPolicySessionScoped(t *testing.T) {
	tests := []struct {
		name     string
		decision interfaces.PermissionDecision
	}{
		{"allow_always", interfaces.PermissionAllowAlways},
		{"allow_session", interfaces.PermissionAllowSession},
	}
	for _, tc := range tests {
		t.Run(tc.name, func(t *testing.T) {
			m := NewManager()

			reqA := interfaces.PermissionRequest{
				SessionID: "sess-A",
				Tool:      "edit_file",
				Target:    "a.go",
				Options:   []interfaces.PermissionDecision{tc.decision, interfaces.PermissionDeny},
			}
			reqB := interfaces.PermissionRequest{
				SessionID: "sess-B",
				Tool:      "edit_file",
				Target:    "a.go",
				Options:   []interfaces.PermissionDecision{tc.decision, interfaces.PermissionDeny},
			}

			// Seed the policy in session A.
			resolveFirstRequest(t, m, reqA, tc.decision)

			// Session B's request must still block — it is a different session.
			ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
			defer cancel()
			start := time.Now()
			_, err := m.Request(ctx, reqB)
			elapsed := time.Since(start)

			if err == nil {
				t.Fatal("expected session B request to block (policy is session-scoped), but it auto-resolved")
			}
			if elapsed < 80*time.Millisecond {
				t.Errorf("expected session B to block ~100ms, returned after %v", elapsed)
			}

			// Clean up.
			pending := m.GetPending()
			if len(pending) == 1 {
				_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
			}
		})
	}
}

// TestClearSessionRemovesPolicies verifies that ClearSession drops the cached
// policies for a session so subsequent requests block again.
func TestClearSessionRemovesPolicies(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "sess-clear",
		Tool:      "edit_file",
		Target:    "main.go",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowAlways, interfaces.PermissionDeny},
	}

	// Seed the policy.
	resolveFirstRequest(t, m, req, interfaces.PermissionAllowAlways)

	// Confirm it auto-resolves.
	done := make(chan interfaces.PermissionDecision, 1)
	go func() {
		d, _ := m.Request(context.Background(), req)
		done <- d
	}()
	select {
	case d := <-done:
		if d != interfaces.PermissionAllowAlways {
			t.Fatalf("expected auto-resolve before clear, got %s", d)
		}
	case <-time.After(time.Second):
		t.Fatal("expected auto-resolve before clear")
	}

	// Clear the session's policies.
	m.ClearSession("sess-clear")

	// Now the request must block again.
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	start := time.Now()
	_, err := m.Request(ctx, req)
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("expected request to block after ClearSession, but it auto-resolved")
	}
	if elapsed < 80*time.Millisecond {
		t.Errorf("expected request to block ~100ms after clear, returned after %v", elapsed)
	}

	// Clean up.
	pending := m.GetPending()
	if len(pending) == 1 {
		_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
	}
}

// TestAutoResolvedDecisionRecordedInAuditLog verifies that auto-resolved
// decisions (served from the policy map) still appear in the audit log.
func TestAutoResolvedDecisionRecordedInAuditLog(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "sess-audit",
		Tool:      "edit_file",
		Target:    "config.json",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowAlways, interfaces.PermissionDeny},
	}

	// Seed via the blocking path (records one audit entry).
	resolveFirstRequest(t, m, req, interfaces.PermissionAllowAlways)

	before := len(m.GetAuditLog())

	// Auto-resolve a second time — this must also record an audit entry.
	if _, err := m.Request(context.Background(), req); err != nil {
		t.Fatalf("auto-resolved request error: %v", err)
	}

	log := m.GetAuditLog()
	if len(log) != before+1 {
		t.Fatalf("expected audit log to grow by 1 for auto-resolved decision, got %d entries (was %d)", len(log), before)
	}

	entry := log[len(log)-1]
	if entry.Decision != string(interfaces.PermissionAllowAlways) {
		t.Errorf("expected last audit decision allow_always, got %s", entry.Decision)
	}
	if entry.SessionID != "sess-audit" {
		t.Errorf("expected last audit sessionID 'sess-audit', got %s", entry.SessionID)
	}
	if entry.Tool != "edit_file" {
		t.Errorf("expected last audit tool 'edit_file', got %s", entry.Tool)
	}
}

// TestPolicyShellCommandBypass verifies that an allow_always decision for one
// shell command does NOT auto-approve a different shell command in the same
// session. Shell commands have no location (target == ""), so previously they
// all shared the empty-target policy key and a single allow_always bypassed
// every subsequent shell command. The fix incorporates the command text into
// the key for shell tools.
func TestPolicyShellCommandBypass(t *testing.T) {
	m := NewManager()

	// Seed an allow_always for "go test" in the session.
	first := interfaces.PermissionRequest{
		SessionID: "sess-shell-bypass",
		Tool:      "execute",
		Command:   "go test",
		Target:    "",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowAlways, interfaces.PermissionDeny},
	}
	if d := resolveFirstRequest(t, m, first, interfaces.PermissionAllowAlways); d != interfaces.PermissionAllowAlways {
		t.Fatalf("first request: expected allow_always, got %s", d)
	}

	// A different command in the same session must NOT auto-resolve — it has a
	// different command text and therefore a different policy key.
	second := interfaces.PermissionRequest{
		SessionID: "sess-shell-bypass",
		Tool:      "execute",
		Command:   "rm -rf /",
		Target:    "",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowAlways, interfaces.PermissionDeny},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	start := time.Now()
	_, err := m.Request(ctx, second)
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("expected different shell command to block (not auto-resolved), but it returned without error")
	}
	if elapsed < 80*time.Millisecond {
		t.Errorf("expected different shell command to block ~100ms, returned after %v", elapsed)
	}

	// Clean up the pending request so the goroutine exits.
	pending := m.GetPending()
	if len(pending) == 1 {
		_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
	}
}

// TestPolicySameShellCommandAutoResolves verifies that the SAME shell command
// still auto-resolves after an allow_always — the command-text keying does not
// break the legitimate auto-approve path.
func TestPolicySameShellCommandAutoResolves(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "sess-shell-same",
		Tool:      "execute",
		Command:   "npm test",
		Target:    "",
		Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowSession, interfaces.PermissionDeny},
	}
	if d := resolveFirstRequest(t, m, req, interfaces.PermissionAllowSession); d != interfaces.PermissionAllowSession {
		t.Fatalf("first request: expected allow_session, got %s", d)
	}

	done := make(chan interfaces.PermissionDecision, 1)
	go func() {
		d, _ := m.Request(context.Background(), req)
		done <- d
	}()
	select {
	case d := <-done:
		if d != interfaces.PermissionAllowSession {
			t.Errorf("expected same shell command to auto-resolve allow_session, got %s", d)
		}
	case <-time.After(time.Second):
		t.Fatal("same shell command did not auto-resolve (blocked)")
	}
}

// TestCleanupStaleDeniesExpiredRequest verifies that CleanupStale denies and
// removes a pending request whose CreatedAt is older than
// pendingRequestTimeout, unblocking the agent goroutine waiting in Request with
// a PermissionDeny. This models the reconnection scenario: a device drops
// Wi-Fi mid-session, the prompt's context is cancelled while disconnected, and
// the prompt would otherwise linger in `pending` forever. The waiting
// goroutine must unblock (with a 2s test timeout so the test can never hang).
func TestCleanupStaleDeniesExpiredRequest(t *testing.T) {
	m := NewManager()

	resultCh := make(chan interfaces.PermissionDecision, 1)
	errCh := make(chan error, 1)
	go func() {
		// Use a long-lived context so the only thing that unblocks Request is
		// CleanupStale sending the deny (not context cancellation).
		d, err := m.Request(context.Background(), interfaces.PermissionRequest{
			SessionID: "sess-stale",
			Tool:      "execute",
			Command:   "rm -rf /tmp/x",
			Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowOnce, interfaces.PermissionDeny},
		})
		if err != nil {
			errCh <- err
			return
		}
		resultCh <- d
	}()

	// Wait for the request to register as pending.
	pending := waitForPending(t, m, 1)

	// Backdate the pending entry so it appears stale to CleanupStale, without
	// actually waiting pendingRequestTimeout (5 minutes) in the test.
	m.mu.Lock()
	if p, ok := m.pending[pending[0].ID]; ok {
		p.CreatedAt = time.Now().Add(-pendingRequestTimeout - time.Second)
	}
	m.mu.Unlock()

	// CleanupStale should deny and remove the stale prompt.
	m.CleanupStale()

	// The pending map should now be empty.
	if got := m.GetPending(); len(got) != 0 {
		t.Fatalf("expected 0 pending after CleanupStale, got %d", len(got))
	}

	// The blocked Request goroutine must unblock with PermissionDeny.
	select {
	case d := <-resultCh:
		if d != interfaces.PermissionDeny {
			t.Errorf("expected PermissionDeny from stale cleanup, got %s", d)
		}
	case err := <-errCh:
		t.Fatalf("request returned unexpected error: %v", err)
	case <-time.After(2 * time.Second):
		t.Fatal("stale request was not unblocked by CleanupStale (timed out)")
	}
}

// TestCleanupStaleKeepsFreshRequest verifies that CleanupStale does NOT touch a
// pending request that is younger than pendingRequestTimeout — only stale
// prompts are pruned.
func TestCleanupStaleKeepsFreshRequest(t *testing.T) {
	m := NewManager()

	go func() {
		_, _ = m.Request(context.Background(), interfaces.PermissionRequest{
			SessionID: "sess-fresh",
			Tool:      "execute",
			Command:   "echo hi",
			Options:   []interfaces.PermissionDecision{interfaces.PermissionAllowOnce, interfaces.PermissionDeny},
		})
	}()

	pending := waitForPending(t, m, 1)

	// CleanupStale on a fresh request should leave it intact.
	m.CleanupStale()
	if got := m.GetPending(); len(got) != 1 {
		t.Fatalf("expected 1 pending after CleanupStale on fresh request, got %d", len(got))
	}

	// Resolve so the Request goroutine exits cleanly.
	_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
}

// TestPolicyRejectAlwaysAutoDenies verifies that a reject_always decision for a
// (session, tool, target) combination auto-denies a subsequent identical request
// without blocking or invoking the callback. Mirrors the allow_always
// auto-resolve path but for the deny side.
func TestPolicyRejectAlwaysAutoDenies(t *testing.T) {
	m := NewManager()

	// Track callback invocations — the second request must NOT trigger it.
	callbackCount := 0
	m.SetCallback(func(_ interfaces.PermissionRequest) {
		callbackCount++
	})

	req := interfaces.PermissionRequest{
		SessionID: "sess-reject-always",
		Tool:      "edit_file",
		Target:    "main.go",
		Options:   []interfaces.PermissionDecision{PermissionRejectAlways, interfaces.PermissionDeny},
	}

	// First request blocks and is resolved with reject_always.
	if d := resolveFirstRequest(t, m, req, PermissionRejectAlways); d != PermissionRejectAlways {
		t.Fatalf("first request: expected reject_always, got %s", d)
	}
	if callbackCount != 1 {
		t.Fatalf("expected callback invoked once after first request, got %d", callbackCount)
	}

	// Second identical request must auto-deny immediately (no blocking).
	done := make(chan interfaces.PermissionDecision, 1)
	errCh := make(chan error, 1)
	go func() {
		d, err := m.Request(context.Background(), req)
		if err != nil {
			errCh <- err
			return
		}
		done <- d
	}()

	select {
	case d := <-done:
		if d != interfaces.PermissionDeny {
			t.Errorf("expected auto-denied request to return PermissionDeny, got %s", d)
		}
	case err := <-errCh:
		t.Fatalf("second request error: %v", err)
	case <-time.After(time.Second):
		t.Fatal("second request did not auto-deny (blocked)")
	}

	if callbackCount != 1 {
		t.Errorf("expected callback still invoked once (not for auto-deny), got %d", callbackCount)
	}

	// The auto-deny must be recorded in the audit log.
	log := m.GetAuditLog()
	if len(log) != 2 {
		t.Fatalf("expected 2 audit entries (seed + auto-deny), got %d", len(log))
	}
	if log[1].Decision != string(interfaces.PermissionDeny) {
		t.Errorf("expected auto-deny audit decision 'deny', got %s", log[1].Decision)
	}
}

// TestClearSessionClearsDenyCache verifies that ClearSession drops cached
// reject_always decisions for a session so subsequent requests block again
// (re-prompt the user) instead of auto-denying.
func TestClearSessionClearsDenyCache(t *testing.T) {
	m := NewManager()

	req := interfaces.PermissionRequest{
		SessionID: "sess-clear-deny",
		Tool:      "edit_file",
		Target:    "main.go",
		Options:   []interfaces.PermissionDecision{PermissionRejectAlways, interfaces.PermissionDeny},
	}

	// Seed the deny cache.
	resolveFirstRequest(t, m, req, PermissionRejectAlways)

	// Confirm it auto-denies.
	done := make(chan interfaces.PermissionDecision, 1)
	go func() {
		d, _ := m.Request(context.Background(), req)
		done <- d
	}()
	select {
	case d := <-done:
		if d != interfaces.PermissionDeny {
			t.Fatalf("expected auto-deny before clear, got %s", d)
		}
	case <-time.After(time.Second):
		t.Fatal("expected auto-deny before clear")
	}

	// Clear the session's deny cache.
	m.ClearSession("sess-clear-deny")

	// Now the request must block again (re-prompt).
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	start := time.Now()
	_, err := m.Request(ctx, req)
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("expected request to block after ClearSession, but it auto-denied")
	}
	if elapsed < 80*time.Millisecond {
		t.Errorf("expected request to block ~100ms after clear, returned after %v", elapsed)
	}

	// Clean up the pending request so the goroutine exits.
	pending := m.GetPending()
	if len(pending) == 1 {
		_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
	}
}

// TestRejectAlwaysTargetScoped verifies that a reject_always for one target
// does NOT auto-deny a request for a different target in the same session. The
// deny cache is keyed by (session, tool, target), matching the allow cache.
func TestRejectAlwaysTargetScoped(t *testing.T) {
	m := NewManager()

	first := interfaces.PermissionRequest{
		SessionID: "sess-reject-scope",
		Tool:      "edit_file",
		Target:    "a.go",
		Options:   []interfaces.PermissionDecision{PermissionRejectAlways, interfaces.PermissionDeny},
	}
	if d := resolveFirstRequest(t, m, first, PermissionRejectAlways); d != PermissionRejectAlways {
		t.Fatalf("first request: expected reject_always, got %s", d)
	}

	// A different target in the same session must NOT auto-deny — it has a
	// different policy key.
	second := interfaces.PermissionRequest{
		SessionID: "sess-reject-scope",
		Tool:      "edit_file",
		Target:    "b.go",
		Options:   []interfaces.PermissionDecision{PermissionRejectAlways, interfaces.PermissionDeny},
	}
	ctx, cancel := context.WithTimeout(context.Background(), 100*time.Millisecond)
	defer cancel()
	start := time.Now()
	_, err := m.Request(ctx, second)
	elapsed := time.Since(start)

	if err == nil {
		t.Fatal("expected different target to block (not auto-denied), but it returned without error")
	}
	if elapsed < 80*time.Millisecond {
		t.Errorf("expected different target to block ~100ms, returned after %v", elapsed)
	}

	// Clean up the pending request so the goroutine exits.
	pending := m.GetPending()
	if len(pending) == 1 {
		_ = m.Respond(context.Background(), pending[0].ID, interfaces.PermissionDeny)
	}
}
