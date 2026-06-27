// Package pairing implements device pairing and authentication.
// Blueprint references: Sec 19 (Authentication).
//
// Two pairing flows:
// 1. First device: QR code with URL + one-time token
// 2. Additional devices: four-word mnemonic passcode
//
// Pairing sessions are short-lived and single-use. Device credentials are
// unique, revocable, and stored in the browser.
package pairing

import (
	"crypto/rand"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"math/big"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	qrcode "github.com/skip2/go-qrcode"
)

// wordList is used to generate four-word mnemonic passcodes.
// A subset of the BIP-39 word list for memorable passcodes.
var wordList = []string{
	"alpha", "bravo", "charlie", "delta", "echo", "foxtrot", "golf",
	"hotel", "india", "juliet", "kilo", "lima", "mike", "november",
	"oscar", "papa", "quebec", "romeo", "sierra", "tango", "uniform",
	"victor", "whiskey", "xray", "yankee", "zulu",
	"purple", "orange", "silver", "golden", "crimson", "azure",
	"wave", "river", "mountain", "forest", "ocean", "desert",
	"fox", "wolf", "bear", "eagle", "hawk", "lion", "tiger",
	"spark", "flame", "ember", "blaze", "storm", "thunder",
}

// PairingSession represents a short-lived, single-use pairing session.
//
//nolint:revive // intentional name for clarity in API responses
type PairingSession struct {
	ID        string    `json:"id"`
	Token     string    `json:"token"`
	Passcode  string    `json:"passcode"`
	URL       string    `json:"url"`
	QRPath    string    `json:"qrPath"`
	CreatedAt time.Time `json:"createdAt"`
	ExpiresAt time.Time `json:"expiresAt"`
	Used      bool      `json:"used"`
}

// DeviceCredential is a long-lived credential issued to a paired device.
type DeviceCredential struct {
	ID       string    `json:"id"`
	Name     string    `json:"name"`
	Secret   string    `json:"secret"`
	PairedAt time.Time `json:"pairedAt"`
}

// Manager handles pairing sessions and device credentials.
type Manager struct {
	mu       sync.Mutex
	sessions map[string]*PairingSession
	devices  map[string]*DeviceCredential
	dataDir  string
	ttl      time.Duration
}

// NewManager creates a new pairing Manager with the given data directory.
// The default pairing session TTL is 5 minutes; override it with SetTTL.
func NewManager(dataDir string) *Manager {
	return &Manager{
		sessions: make(map[string]*PairingSession),
		devices:  make(map[string]*DeviceCredential),
		dataDir:  dataDir,
		ttl:      5 * time.Minute,
	}
}

// SetTTL sets the pairing session time-to-live. This should be called before
// any sessions are created.
func (m *Manager) SetTTL(ttl time.Duration) {
	m.mu.Lock()
	defer m.mu.Unlock()
	m.ttl = ttl
}

// CreateSession generates a new pairing session with a QR code and mnemonic.
// The session expires after the configured TTL (default 5 minutes) and can
// only be used once.
func (m *Manager) CreateSession(host string, port int) (*PairingSession, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	token, err := generateToken(32)
	if err != nil {
		return nil, fmt.Errorf("generate token: %w", err)
	}

	passcode, err := generatePasscode(4)
	if err != nil {
		return nil, fmt.Errorf("generate passcode: %w", err)
	}

	sessionID, err := generateToken(16)
	if err != nil {
		return nil, fmt.Errorf("generate session id: %w", err)
	}

	url := fmt.Sprintf("http://%s:%d?token=%s", host, port, token)

	// Generate QR code PNG.
	qrPath := filepath.Join(m.dataDir, fmt.Sprintf("pairing-%s.png", sessionID))
	if err := qrcode.WriteFile(url, qrcode.Medium, 256, qrPath); err != nil {
		return nil, fmt.Errorf("generate qr code: %w", err)
	}

	ttl := m.ttl
	if ttl == 0 {
		ttl = 5 * time.Minute
	}
	session := &PairingSession{
		ID:        sessionID,
		Token:     token,
		Passcode:  passcode,
		URL:       url,
		QRPath:    qrPath,
		CreatedAt: time.Now().UTC(),
		ExpiresAt: time.Now().UTC().Add(ttl),
		Used:      false,
	}

	m.sessions[sessionID] = session
	return session, nil
}

// VerifyPasscode validates a mnemonic passcode and issues a device credential.
// The passcode must match an active, unused, non-expired pairing session.
func (m *Manager) VerifyPasscode(passcode, deviceName string) (*DeviceCredential, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	session := m.findSession(func(s *PairingSession) bool {
		return s.Passcode == passcode
	})
	if session == nil {
		return nil, fmt.Errorf("invalid or expired passcode")
	}
	return m.issueCredential(session, deviceName)
}

// VerifyToken validates a QR code token and issues a device credential.
func (m *Manager) VerifyToken(token, deviceName string) (*DeviceCredential, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	session := m.findSession(func(s *PairingSession) bool {
		return s.Token == token
	})
	if session == nil {
		return nil, fmt.Errorf("invalid or expired token")
	}
	return m.issueCredential(session, deviceName)
}

// findSession returns the first unused, non-expired session matching match,
// or nil if none is found. Caller must hold m.mu.
func (m *Manager) findSession(match func(*PairingSession) bool) *PairingSession {
	now := time.Now().UTC()
	for _, s := range m.sessions {
		if match(s) && !s.Used && now.Before(s.ExpiresAt) {
			return s
		}
	}
	return nil
}

// issueCredential marks the session used, generates a device credential, and
// cleans up the QR code file. Caller must hold m.mu.
func (m *Manager) issueCredential(session *PairingSession, deviceName string) (*DeviceCredential, error) {
	session.Used = true

	credID, err := generateToken(16)
	if err != nil {
		return nil, fmt.Errorf("generate credential id: %w", err)
	}

	secret, err := generateToken(32)
	if err != nil {
		return nil, fmt.Errorf("generate secret: %w", err)
	}

	cred := &DeviceCredential{
		ID:       credID,
		Name:     deviceName,
		Secret:   secret,
		PairedAt: time.Now().UTC(),
	}

	m.devices[credID] = cred
	_ = os.Remove(session.QRPath)

	return cred, nil
}

// ValidateCredential checks whether a device credential is valid.
func (m *Manager) ValidateCredential(deviceID, secret string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()

	cred, ok := m.devices[deviceID]
	if !ok {
		return false
	}
	return cred.Secret == secret
}

// ListDevices returns all paired device credentials.
func (m *Manager) ListDevices() []DeviceCredential {
	m.mu.Lock()
	defer m.mu.Unlock()

	devices := make([]DeviceCredential, 0, len(m.devices))
	for _, cred := range m.devices {
		devices = append(devices, *cred)
	}
	return devices
}

// RevokeDevice removes a device's credential, preventing further access.
func (m *Manager) RevokeDevice(deviceID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if _, ok := m.devices[deviceID]; !ok {
		return fmt.Errorf("device not found: %s", deviceID)
	}

	delete(m.devices, deviceID)
	return nil
}

// generateToken generates a cryptographically random hex string of the given byte length.
func generateToken(byteLen int) (string, error) {
	b := make([]byte, byteLen)
	if _, err := rand.Read(b); err != nil {
		return "", err
	}
	return hex.EncodeToString(b), nil
}

// generatePasscode generates a hyphenated mnemonic passcode with the given word count.
func generatePasscode(wordCount int) (string, error) {
	words := make([]string, wordCount)
	for i := 0; i < wordCount; i++ {
		n, err := rand.Int(rand.Reader, big.NewInt(int64(len(wordList))))
		if err != nil {
			return "", err
		}
		words[i] = wordList[n.Int64()]
	}
	return strings.Join(words, "-"), nil
}

// HashSecret returns a SHA-256 hash of a secret for safe comparison/storage.
func HashSecret(secret string) string {
	h := sha256.Sum256([]byte(secret))
	return hex.EncodeToString(h[:])
}
