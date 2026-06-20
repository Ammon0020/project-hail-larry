package pairing

import (
	"strings"
	"testing"
)

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
