package pairing

import (
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

// setLastSeen is an unexported test helper that directly overwrites a device's
// LastSeen timestamp so inactivity-expiry behavior can be exercised
// deterministically without real sleeps. It holds the manager lock like
// production callers.
func (m *Manager) setLastSeen(deviceID string, t time.Time) {
	m.mu.Lock()
	defer m.mu.Unlock()
	if d, ok := m.devices[deviceID]; ok {
		d.LastSeen = t
	}
}

// getLastSeen is an unexported test helper returning the stored LastSeen for a
// device (zero time if absent).
func (m *Manager) getLastSeen(deviceID string) time.Time {
	m.mu.Lock()
	defer m.mu.Unlock()
	if d, ok := m.devices[deviceID]; ok {
		return d.LastSeen
	}
	return time.Time{}
}

// pairDevice is a small helper that creates a session and pairs a device,
// returning the issued credential.
func pairDevice(t *testing.T, m *Manager) *DeviceCredential {
	t.Helper()
	session, err := m.CreateSession("localhost", 7337)
	if err != nil {
		t.Fatalf("create session: %v", err)
	}
	cred, err := m.VerifyPasscode(session.Passcode, "Device")
	if err != nil {
		t.Fatalf("verify passcode: %v", err)
	}
	return cred
}

// newTestManager creates a pairing Manager with a temp directory.
func newTestManager(t *testing.T) *Manager {
	t.Helper()
	return NewManager(t.TempDir())
}

// TestCreateSession verifies that a pairing session is created with all required fields.
func TestCreateSession(t *testing.T) {
	m := newTestManager(t)

	session, err := m.CreateSession("192.168.1.100", 7337)
	if err != nil {
		t.Fatalf("create session: %v", err)
	}

	if session.ID == "" {
		t.Error("expected non-empty session ID")
	}
	if session.Token == "" {
		t.Error("expected non-empty token")
	}
	if session.Passcode == "" {
		t.Error("expected non-empty passcode")
	}
	if !strings.Contains(session.URL, "192.168.1.100:7337") {
		t.Errorf("expected URL to contain host:port, got %s", session.URL)
	}
	if !strings.Contains(session.URL, session.Token) {
		t.Errorf("expected URL to contain token, got %s", session.URL)
	}

	// Passcode should be 4 hyphen-separated words.
	words := strings.Split(session.Passcode, "-")
	if len(words) != 4 {
		t.Errorf("expected 4 words in passcode, got %d: %s", len(words), session.Passcode)
	}
}

// TestVerifyPasscode verifies that a valid passcode issues a device credential.
func TestVerifyPasscode(t *testing.T) {
	m := newTestManager(t)

	session, err := m.CreateSession("localhost", 7337)
	if err != nil {
		t.Fatalf("create session: %v", err)
	}

	cred, err := m.VerifyPasscode(session.Passcode, "iPhone")
	if err != nil {
		t.Fatalf("verify passcode: %v", err)
	}

	if cred.ID == "" {
		t.Error("expected non-empty credential ID")
	}
	if cred.Secret == "" {
		t.Error("expected non-empty secret")
	}
	if cred.Name != "iPhone" {
		t.Errorf("expected name 'iPhone', got %s", cred.Name)
	}
}

// TestVerifyPasscodeInvalid verifies that an invalid passcode is rejected.
func TestVerifyPasscodeInvalid(t *testing.T) {
	m := newTestManager(t)

	_, err := m.VerifyPasscode("invalid-passcode-here", "Device")
	if err == nil {
		t.Error("expected error for invalid passcode")
	}
}

// TestVerifyPasscodeSingleUse verifies that a passcode can only be used once.
func TestVerifyPasscodeSingleUse(t *testing.T) {
	m := newTestManager(t)

	session, err := m.CreateSession("localhost", 7337)
	if err != nil {
		t.Fatalf("create session: %v", err)
	}

	// First use should succeed.
	_, err = m.VerifyPasscode(session.Passcode, "Device1")
	if err != nil {
		t.Fatalf("first verify: %v", err)
	}

	// Second use should fail.
	_, err = m.VerifyPasscode(session.Passcode, "Device2")
	if err == nil {
		t.Error("expected error for reused passcode")
	}
}

// TestVerifyToken verifies that a QR token issues a device credential.
func TestVerifyToken(t *testing.T) {
	m := newTestManager(t)

	session, err := m.CreateSession("localhost", 7337)
	if err != nil {
		t.Fatalf("create session: %v", err)
	}

	cred, err := m.VerifyToken(session.Token, "MacBook")
	if err != nil {
		t.Fatalf("verify token: %v", err)
	}

	if cred.Name != "MacBook" {
		t.Errorf("expected name 'MacBook', got %s", cred.Name)
	}
}

// TestValidateCredential verifies that issued credentials are valid.
func TestValidateCredential(t *testing.T) {
	m := newTestManager(t)

	session, _ := m.CreateSession("localhost", 7337)
	cred, _ := m.VerifyPasscode(session.Passcode, "Device")

	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected credential to be valid")
	}

	if m.ValidateCredential(cred.ID, "wrong-secret") {
		t.Error("expected credential with wrong secret to be invalid")
	}

	if m.ValidateCredential("nonexistent", cred.Secret) {
		t.Error("expected nonexistent device to be invalid")
	}
}

// TestRevokeDevice verifies that revocation removes access.
func TestRevokeDevice(t *testing.T) {
	m := newTestManager(t)

	session, _ := m.CreateSession("localhost", 7337)
	cred, _ := m.VerifyPasscode(session.Passcode, "Device")

	// Should be valid before revocation.
	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected credential to be valid before revocation")
	}

	// Revoke.
	if err := m.RevokeDevice(cred.ID); err != nil {
		t.Fatalf("revoke: %v", err)
	}

	// Should be invalid after revocation.
	if m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected credential to be invalid after revocation")
	}
}

// TestRevokeDeviceNotFound verifies revoking a nonexistent device returns an error.
func TestRevokeDeviceNotFound(t *testing.T) {
	m := newTestManager(t)

	err := m.RevokeDevice("nonexistent")
	if err == nil {
		t.Error("expected error for revoking nonexistent device")
	}
}

// TestListDevices verifies that all paired devices are listed.
func TestListDevices(t *testing.T) {
	m := newTestManager(t)

	s1, _ := m.CreateSession("localhost", 7337)
	m.VerifyPasscode(s1.Passcode, "Device1")

	s2, _ := m.CreateSession("localhost", 7337)
	m.VerifyPasscode(s2.Passcode, "Device2")

	devices := m.ListDevices()
	if len(devices) != 2 {
		t.Fatalf("expected 2 devices, got %d", len(devices))
	}
}

// TestValidateCredentialRenewsWithinWindow verifies that a credential validates
// within the inactivity window and that a successful validation advances
// (renews) LastSeen.
func TestValidateCredentialRenewsWithinWindow(t *testing.T) {
	m := newTestManager(t)
	m.SetInactivityTTL(time.Hour)
	cred := pairDevice(t, m)

	// Push LastSeen back to just inside the window, then validate. The
	// validation should succeed and renew LastSeen to (approximately) now.
	old := time.Now().UTC().Add(-30 * time.Minute)
	m.setLastSeen(cred.ID, old)

	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Fatal("expected credential to be valid within the window")
	}

	renewed := m.getLastSeen(cred.ID)
	if !renewed.After(old) {
		t.Errorf("expected LastSeen to be renewed after validation: old=%v new=%v", old, renewed)
	}
}

// TestValidateCredentialExpiresAfterInactivity verifies that a credential idle
// longer than the inactivity TTL fails validation and is NOT auto-deleted.
func TestValidateCredentialExpiresAfterInactivity(t *testing.T) {
	m := newTestManager(t)
	m.SetInactivityTTL(time.Hour)
	cred := pairDevice(t, m)

	// Simulate inactivity well beyond the TTL.
	m.setLastSeen(cred.ID, time.Now().UTC().Add(-2*time.Hour))

	if m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected expired credential to be invalid")
	}

	// The device must remain in the map (ValidateCredential does not delete).
	if len(m.ListDevices()) != 1 {
		t.Errorf("expected expired device to remain listed, got %d devices", len(m.ListDevices()))
	}
}

// TestValidateCredentialSlidingWindow verifies that ValidateCredential renews
// LastSeen on each successful call, so a credential stays valid as long as the
// gap between validations stays under the TTL — even when the total elapsed
// time since pairing exceeds the TTL. Uses a short TTL and real sleeps so the
// renewal (not manual setLastSeen) is what keeps the window alive.
func TestValidateCredentialSlidingWindow(t *testing.T) {
	m := newTestManager(t)
	m.SetInactivityTTL(200 * time.Millisecond)
	cred := pairDevice(t, m)

	// Validate at t=0 — renews LastSeen to now.
	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Fatal("expected valid immediately after pairing")
	}
	// 100ms later (within TTL of the renewal) — should renew again.
	time.Sleep(100 * time.Millisecond)
	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Fatal("expected valid 100ms after renewal (< 200ms TTL)")
	}
	// 300ms later (> TTL since last renewal) — should be expired.
	time.Sleep(300 * time.Millisecond)
	if m.ValidateCredential(cred.ID, cred.Secret) {
		t.Fatal("expected expired after 300ms > 200ms TTL since last renewal")
	}
}

// TestValidateCredentialDisabledNeverExpires verifies that inactivityTTL == 0
// disables expiry entirely, even for a credential with an ancient LastSeen.
func TestValidateCredentialDisabledNeverExpires(t *testing.T) {
	m := newTestManager(t)
	// Default TTL is 0 (disabled); be explicit for clarity.
	m.SetInactivityTTL(0)
	cred := pairDevice(t, m)

	m.setLastSeen(cred.ID, time.Now().UTC().Add(-100*24*time.Hour)) // 100 days idle

	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected credential to remain valid when expiry is disabled")
	}
}

// TestLoadDevicesMigratesLastSeen verifies that a persisted device with a zero
// LastSeen (written before the field existed) gets LastSeen backfilled to the
// current time on load, so legacy devices receive a fresh full window from
// upgrade time rather than being instantly expired if PairedAt is older than
// the TTL.
func TestLoadDevicesMigratesLastSeen(t *testing.T) {
	dir := t.TempDir()

	pairedAt := time.Now().UTC().Add(-24 * time.Hour).Round(time.Second)
	// Write a legacy record with no lastSeen field.
	legacy := []map[string]any{
		{
			"id":         "legacy-device",
			"name":       "Legacy",
			"secretHash": HashSecret("some-secret"),
			"pairedAt":   pairedAt.Format(time.RFC3339Nano),
		},
	}
	data, err := json.MarshalIndent(legacy, "", "  ")
	if err != nil {
		t.Fatalf("marshal legacy: %v", err)
	}
	if err := os.WriteFile(filepath.Join(dir, devicesFileName), data, 0600); err != nil {
		t.Fatalf("write legacy devices file: %v", err)
	}

	before := time.Now().UTC()
	m := NewManager(dir)
	after := time.Now().UTC()

	got := m.getLastSeen("legacy-device")
	if got.Before(before) || got.After(after) {
		t.Errorf("expected migrated LastSeen to be ~now (upgrade time), got %v; before=%v after=%v", got, before, after)
	}
}

// TestNewCredentialSeedsLastSeen verifies that a freshly issued credential has
// LastSeen initialized (non-zero) so it starts with a full window.
func TestNewCredentialSeedsLastSeen(t *testing.T) {
	m := newTestManager(t)
	cred := pairDevice(t, m)
	if m.getLastSeen(cred.ID).IsZero() {
		t.Error("expected newly issued credential to have a non-zero LastSeen")
	}
}

// ----------------------------------------------------------------------------
// Grace-period pending action tests (Blueprint Sec 19, Sec 13)
// ----------------------------------------------------------------------------

// TestRequestRevocation verifies that requesting a revocation with a positive
// grace period creates a pending action that appears in ListPendingActions and
// does not immediately delete the device.
func TestRequestRevocation(t *testing.T) {
	m := newTestManager(t)
	cred := pairDevice(t, m)

	action, err := m.RequestRevocation(cred.ID, "requester-dev", 5*time.Minute)
	if err != nil {
		t.Fatalf("request revocation: %v", err)
	}
	if action == nil {
		t.Fatal("expected non-nil pending action")
	}
	if action.ID == "" {
		t.Error("expected non-empty action ID")
	}
	if action.Type != PendingActionTypeRevocation {
		t.Errorf("expected type %q, got %q", PendingActionTypeRevocation, action.Type)
	}
	if action.DeviceID != cred.ID {
		t.Errorf("expected deviceID %s, got %s", cred.ID, action.DeviceID)
	}
	if action.RequestedBy != "requester-dev" {
		t.Errorf("expected requestedBy 'requester-dev', got %s", action.RequestedBy)
	}

	// The device must still be present (grace period has not elapsed).
	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected device to remain valid during grace period")
	}

	// The pending action must appear in ListPendingActions.
	pending := m.ListPendingActions()
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending action, got %d", len(pending))
	}
	if pending[0].ID != action.ID {
		t.Errorf("expected pending ID %s, got %s", action.ID, pending[0].ID)
	}
}

// TestCancelRevocation verifies that cancelling a pending revocation stops the
// timer and removes the pending action, leaving the device intact.
func TestCancelRevocation(t *testing.T) {
	m := newTestManager(t)
	cred := pairDevice(t, m)

	action, err := m.RequestRevocation(cred.ID, "requester-dev", 5*time.Minute)
	if err != nil {
		t.Fatalf("request revocation: %v", err)
	}

	if err := m.CancelRevocation(action.ID); err != nil {
		t.Fatalf("cancel revocation: %v", err)
	}

	// The pending action must be gone.
	if pending := m.ListPendingActions(); len(pending) != 0 {
		t.Errorf("expected 0 pending actions after cancel, got %d", len(pending))
	}

	// The device must still be valid.
	if !m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected device to remain valid after cancelled revocation")
	}
}

// TestCancelRevocationNotFound verifies that cancelling a non-existent action
// returns an error.
func TestCancelRevocationNotFound(t *testing.T) {
	m := newTestManager(t)
	if err := m.CancelRevocation("nonexistent"); err == nil {
		t.Error("expected error for cancelling non-existent action")
	}
}

// TestRevocationGracePeriodExpires verifies that when the grace period elapses
// the revocation fires and the device is deleted. Uses a short grace period
// with a real sleep.
func TestRevocationGracePeriodExpires(t *testing.T) {
	m := newTestManager(t)
	cred := pairDevice(t, m)

	if _, err := m.RequestRevocation(cred.ID, "requester-dev", 50*time.Millisecond); err != nil {
		t.Fatalf("request revocation: %v", err)
	}

	// Wait for the timer to fire.
	time.Sleep(150 * time.Millisecond)

	// The device must be revoked.
	if m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected device to be revoked after grace period expired")
	}

	// The pending action must be cleaned up.
	if pending := m.ListPendingActions(); len(pending) != 0 {
		t.Errorf("expected 0 pending actions after expiry, got %d", len(pending))
	}
}

// TestImmediateRevocation verifies that a grace period of 0 revokes the device
// immediately (backward compatible with the pre-grace-period RevokeDevice).
func TestImmediateRevocation(t *testing.T) {
	m := newTestManager(t)
	cred := pairDevice(t, m)

	action, err := m.RequestRevocation(cred.ID, "requester-dev", 0)
	if err != nil {
		t.Fatalf("request revocation: %v", err)
	}
	if action == nil {
		t.Fatal("expected non-nil action even on immediate revocation")
	}

	// The device must be revoked immediately.
	if m.ValidateCredential(cred.ID, cred.Secret) {
		t.Error("expected device to be revoked immediately when gracePeriod=0")
	}

	// No pending action should remain.
	if pending := m.ListPendingActions(); len(pending) != 0 {
		t.Errorf("expected 0 pending actions after immediate revocation, got %d", len(pending))
	}
}

// TestDuplicatePendingRevocation verifies that requesting revocation for a
// device that already has a pending revocation returns an error, so a stolen
// device cannot queue many overlapping timers.
func TestDuplicatePendingRevocation(t *testing.T) {
	m := newTestManager(t)
	cred := pairDevice(t, m)

	if _, err := m.RequestRevocation(cred.ID, "requester-dev", 5*time.Minute); err != nil {
		t.Fatalf("first request: %v", err)
	}
	if _, err := m.RequestRevocation(cred.ID, "requester-dev", 5*time.Minute); err == nil {
		t.Error("expected error for duplicate pending revocation")
	}
}

// TestRequestRevocationUnknownDevice verifies that requesting revocation for a
// non-existent device returns an error.
func TestRequestRevocationUnknownDevice(t *testing.T) {
	m := newTestManager(t)
	if _, err := m.RequestRevocation("nonexistent", "requester-dev", 5*time.Minute); err == nil {
		t.Error("expected error for revoking unknown device")
	}
}

// TestRequestWorkspaceRegistration verifies that requesting a workspace
// registration with a positive grace period creates a pending action without
// calling the workspaceRegisterFn.
func TestRequestWorkspaceRegistration(t *testing.T) {
	m := newTestManager(t)
	called := false
	m.SetWorkspaceRegisterFn(func(_ string) error {
		called = true
		return nil
	})

	action, err := m.RequestWorkspaceRegistration("/some/path", "requester-dev", 5*time.Minute)
	if err != nil {
		t.Fatalf("request workspace registration: %v", err)
	}
	if action.Type != PendingActionTypeWorkspaceRegistration {
		t.Errorf("expected type %q, got %q", PendingActionTypeWorkspaceRegistration, action.Type)
	}
	if action.Path != "/some/path" {
		t.Errorf("expected path /some/path, got %s", action.Path)
	}
	if called {
		t.Error("expected workspaceRegisterFn NOT to be called during grace period")
	}

	pending := m.ListPendingActions()
	if len(pending) != 1 {
		t.Fatalf("expected 1 pending action, got %d", len(pending))
	}
}

// TestCancelWorkspaceRegistration verifies that cancelling a pending workspace
// registration removes it without calling workspaceRegisterFn.
func TestCancelWorkspaceRegistration(t *testing.T) {
	m := newTestManager(t)
	called := false
	m.SetWorkspaceRegisterFn(func(_ string) error {
		called = true
		return nil
	})

	action, err := m.RequestWorkspaceRegistration("/some/path", "requester-dev", 5*time.Minute)
	if err != nil {
		t.Fatalf("request: %v", err)
	}
	if err := m.CancelWorkspaceRegistration(action.ID); err != nil {
		t.Fatalf("cancel: %v", err)
	}
	if called {
		t.Error("expected workspaceRegisterFn NOT to be called after cancel")
	}
	if pending := m.ListPendingActions(); len(pending) != 0 {
		t.Errorf("expected 0 pending actions, got %d", len(pending))
	}
}

// TestWorkspaceRegistrationExpires verifies that when the grace period elapses
// the workspaceRegisterFn is called and the pending action is removed.
func TestWorkspaceRegistrationExpires(t *testing.T) {
	m := newTestManager(t)
	called := make(chan string, 1)
	m.SetWorkspaceRegisterFn(func(p string) error {
		called <- p
		return nil
	})

	if _, err := m.RequestWorkspaceRegistration("/some/path", "requester-dev", 50*time.Millisecond); err != nil {
		t.Fatalf("request: %v", err)
	}

	select {
	case path := <-called:
		if path != "/some/path" {
			t.Errorf("expected workspaceRegisterFn called with /some/path, got %s", path)
		}
	case <-time.After(2 * time.Second):
		t.Fatal("expected workspaceRegisterFn to be called after grace period")
	}

	// Give the timer goroutine a moment to finish cleanup, then check the
	// pending map is empty.
	time.Sleep(20 * time.Millisecond)
	if pending := m.ListPendingActions(); len(pending) != 0 {
		t.Errorf("expected 0 pending actions after expiry, got %d", len(pending))
	}
}

// TestDuplicatePendingWorkspaceRegistration verifies that requesting
// registration for a path that already has a pending registration returns an
// error.
func TestDuplicatePendingWorkspaceRegistration(t *testing.T) {
	m := newTestManager(t)
	m.SetWorkspaceRegisterFn(func(_ string) error { return nil })

	if _, err := m.RequestWorkspaceRegistration("/some/path", "requester-dev", 5*time.Minute); err != nil {
		t.Fatalf("first request: %v", err)
	}
	if _, err := m.RequestWorkspaceRegistration("/some/path", "requester-dev", 5*time.Minute); err == nil {
		t.Error("expected error for duplicate pending workspace registration")
	}
}

// TestCancelRevocationTypeMismatch verifies that cancelling a workspace
// registration via CancelRevocation (wrong type) returns an error.
func TestCancelRevocationTypeMismatch(t *testing.T) {
	m := newTestManager(t)
	m.SetWorkspaceRegisterFn(func(_ string) error { return nil })

	action, err := m.RequestWorkspaceRegistration("/some/path", "requester-dev", 5*time.Minute)
	if err != nil {
		t.Fatalf("request: %v", err)
	}
	if err := m.CancelRevocation(action.ID); err == nil {
		t.Error("expected error when cancelling workspace registration via CancelRevocation")
	}
}
