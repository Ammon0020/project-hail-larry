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
	"crypto/subtle"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"log"
	"math/big"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	qrcode "github.com/skip2/go-qrcode"
)

// wordList is defined in words.go (the BIP-39 English word list, 2048 entries).

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

// DeviceCredential is a long-lived credential issued to a paired device. It is
// returned exactly once at pairing time (from VerifyPasscode/VerifyToken) so the
// newly paired device can display and store the raw Secret. The Manager never
// persists the raw Secret; only its SHA-256 hash is retained at rest (see
// storedDevice). List responses use DeviceInfo, which omits all secret material.
type DeviceCredential struct {
	ID       string    `json:"id"`
	Name     string    `json:"name"`
	Secret   string    `json:"secret"`
	PairedAt time.Time `json:"pairedAt"`
}

// DeviceInfo is a public, secret-free view of a paired device. It is used for
// list/admin API responses so that credential material (raw secrets or hashes)
// is never serialized in bulk.
type DeviceInfo struct {
	ID       string    `json:"id"`
	Name     string    `json:"name"`
	PairedAt time.Time `json:"pairedAt"`
}

// storedDevice is the internal at-rest record for a paired device. Only the
// SHA-256 hash of the device secret is kept; the raw secret is discarded after
// it is returned once at issuance time. The JSON tags allow it to be persisted
// to disk (devices.json) so pairings survive daemon restarts.
type storedDevice struct {
	ID         string    `json:"id"`
	Name       string    `json:"name"`
	SecretHash string    `json:"secretHash"`
	PairedAt   time.Time `json:"pairedAt"`
}

// Rate-limiting constants for pairing verification. After maxVerifyAttempts
// failures within rateLimitWindow, subsequent attempts are rejected with an
// exponentially growing lockout (doubling per lockout, capped at
// maxLockout). This makes online brute force of the mnemonic passcode
// infeasible even though the verify endpoints are unauthenticated.
const (
	maxVerifyAttempts = 5
	rateLimitWindow   = 5 * time.Minute
	baseLockout       = 1 * time.Second
	maxLockout        = 5 * time.Minute
)

// Manager handles pairing sessions and device credentials.
type Manager struct {
	mu       sync.Mutex
	sessions map[string]*PairingSession
	devices  map[string]*storedDevice
	dataDir  string
	ttl      time.Duration

	// Rate-limiting state for verify attempts. failures holds timestamps of
	// recent failed attempts within rateLimitWindow; lockoutUntil is the time
	// before which all verify attempts are rejected; lockoutCount tracks the
	// number of lockouts to scale exponential backoff.
	failures     []time.Time
	lockoutUntil time.Time
	lockoutCount int
}

// devicesFileName is the on-disk file (within dataDir) that persists paired
// device credentials. Only SHA-256 hashes of secrets are stored, never raw
// secrets, so the file is safe to persist across restarts.
const devicesFileName = "devices.json"

// NewManager creates a new pairing Manager with the given data directory.
// The default pairing session TTL is 5 minutes; override it with SetTTL.
//
// Device credentials persisted by a previous run are loaded from
// <dataDir>/devices.json so pairings survive daemon restarts. A missing file
// (first run) is not an error. If the file exists but cannot be read or parsed,
// the error is logged loudly and the manager starts with no devices — the
// daemon still runs, but previously paired devices must re-pair.
func NewManager(dataDir string) *Manager {
	m := &Manager{
		sessions: make(map[string]*PairingSession),
		devices:  make(map[string]*storedDevice),
		dataDir:  dataDir,
		ttl:      5 * time.Minute,
	}
	m.loadDevices()
	return m
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

	// Sweep expired/used sessions before inserting a new one so the sessions
	// map and orphaned QR files do not grow without bound over time.
	m.cleanupSessions()

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
// The passcode must match an active, unused, non-expired pairing session. To
// resist brute force, attempts are rate limited: after maxVerifyAttempts
// failures within rateLimitWindow, subsequent attempts are rejected with an
// exponentially growing lockout. A generic error is returned regardless of
// whether no session matched or the session was used/expired, to avoid
// enumerating active sessions.
func (m *Manager) VerifyPasscode(passcode, deviceName string) (*DeviceCredential, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Sweep expired/used sessions on each verify so cleanup happens even when
	// no new sessions are being created.
	m.cleanupSessions()

	if err := m.checkRateLimit(); err != nil {
		return nil, err
	}

	session := m.findSession(func(s *PairingSession) bool {
		// Constant-time comparison prevents timing leakage of the matching
		// prefix of the passcode across many requests.
		return subtle.ConstantTimeCompare([]byte(s.Passcode), []byte(passcode)) == 1
	})
	if session == nil {
		m.recordFailure()
		return nil, fmt.Errorf("invalid or expired passcode")
	}
	cred, err := m.issueCredential(session, deviceName)
	if err != nil {
		return nil, err
	}
	m.resetRateLimit()
	return cred, nil
}

// VerifyToken validates a QR code token and issues a device credential. The
// token must match an active, unused, non-expired pairing session. The same
// rate limiting applied to VerifyPasscode applies here.
func (m *Manager) VerifyToken(token, deviceName string) (*DeviceCredential, error) {
	m.mu.Lock()
	defer m.mu.Unlock()

	// Sweep expired/used sessions on each verify so cleanup happens even when
	// no new sessions are being created.
	m.cleanupSessions()

	if err := m.checkRateLimit(); err != nil {
		return nil, err
	}

	session := m.findSession(func(s *PairingSession) bool {
		// Constant-time comparison prevents timing leakage of the token.
		return subtle.ConstantTimeCompare([]byte(s.Token), []byte(token)) == 1
	})
	if session == nil {
		m.recordFailure()
		return nil, fmt.Errorf("invalid or expired token")
	}
	cred, err := m.issueCredential(session, deviceName)
	if err != nil {
		return nil, err
	}
	m.resetRateLimit()
	return cred, nil
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
// cleans up the QR code file. The raw secret is returned to the caller exactly
// once (so the newly paired device can display/store it), while only its
// SHA-256 hash is retained at rest in m.devices. Caller must hold m.mu.
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

	now := time.Now().UTC()

	// Store only the hash of the secret at rest; never the raw secret.
	stored := &storedDevice{
		ID:         credID,
		Name:       deviceName,
		SecretHash: HashSecret(secret),
		PairedAt:   now,
	}
	m.devices[credID] = stored
	// Persist the new credential so the pairing survives a daemon restart.
	// A persistence failure is returned loudly rather than silently leaving the
	// in-memory credential orphaned on restart.
	if err := m.saveDevices(); err != nil {
		// Roll back the in-memory addition so state stays consistent with disk.
		delete(m.devices, credID)
		return nil, fmt.Errorf("persist device credential: %w", err)
	}
	_ = os.Remove(session.QRPath)
	// The session is single-use; remove it from the map so it cannot be
	// matched again and does not linger until the next sweep.
	delete(m.sessions, session.ID)

	// The returned credential carries the raw secret for one-time display.
	// It is not persisted by the Manager.
	return &DeviceCredential{
		ID:       credID,
		Name:     deviceName,
		Secret:   secret,
		PairedAt: now,
	}, nil
}

// ValidateCredential checks whether a device credential is valid. The supplied
// secret is hashed and compared against the stored hash using
// subtle.ConstantTimeCompare so that neither the length of the matching prefix
// nor whether the deviceID exists is leaked via timing. The signature is kept
// stable so callers (e.g. auth middleware) need no changes after secrets began
// being hashed at rest.
func (m *Manager) ValidateCredential(deviceID, secret string) bool {
	m.mu.Lock()
	defer m.mu.Unlock()

	stored, ok := m.devices[deviceID]
	if !ok {
		// Perform a dummy comparison against a fresh hash so the "unknown
		// device" path takes roughly the same time as the "wrong secret"
		// path, avoiding deviceID enumeration via timing.
		dummy := HashSecret(secret)
		_ = subtle.ConstantTimeCompare([]byte(dummy), []byte(dummy))
		return false
	}
	got := HashSecret(secret)
	return subtle.ConstantTimeCompare([]byte(got), []byte(stored.SecretHash)) == 1
}

// ListDevices returns a secret-free view of all paired devices. Neither the
// raw secret nor its hash is ever included, so list/admin API responses cannot
// leak credential material.
func (m *Manager) ListDevices() []DeviceInfo {
	m.mu.Lock()
	defer m.mu.Unlock()

	devices := make([]DeviceInfo, 0, len(m.devices))
	for _, stored := range m.devices {
		devices = append(devices, DeviceInfo{
			ID:       stored.ID,
			Name:     stored.Name,
			PairedAt: stored.PairedAt,
		})
	}
	return devices
}

// RevokeDevice removes a device's credential, preventing further access. The
// revocation is persisted to disk so the device cannot re-authenticate after a
// daemon restart.
func (m *Manager) RevokeDevice(deviceID string) error {
	m.mu.Lock()
	defer m.mu.Unlock()

	if _, ok := m.devices[deviceID]; !ok {
		return fmt.Errorf("device not found: %s", deviceID)
	}

	delete(m.devices, deviceID)
	// Persist the removal so the device stays revoked across restarts.
	if err := m.saveDevices(); err != nil {
		return fmt.Errorf("persist device revocation: %w", err)
	}
	return nil
}

// checkRateLimit returns an error if verify attempts are currently locked out.
// It also prunes failure timestamps that fall outside the rate-limit window.
// Caller must hold m.mu.
func (m *Manager) checkRateLimit() error {
	now := time.Now().UTC()

	// Drop failures outside the sliding window.
	cutoff := now.Add(-rateLimitWindow)
	pruned := m.failures[:0]
	for _, t := range m.failures {
		if t.After(cutoff) {
			pruned = append(pruned, t)
		}
	}
	m.failures = pruned

	if now.Before(m.lockoutUntil) {
		return fmt.Errorf("too many attempts, try again in %s", m.lockoutUntil.Sub(now).Round(time.Second))
	}
	return nil
}

// recordFailure logs a failed verify attempt and, once the attempt threshold is
// reached within the window, engages an exponentially growing lockout. Caller
// must hold m.mu.
func (m *Manager) recordFailure() {
	now := time.Now().UTC()
	m.failures = append(m.failures, now)

	if len(m.failures) >= maxVerifyAttempts {
		// Exponential backoff: baseLockout * 2^lockoutCount, capped at maxLockout.
		backoff := baseLockout << uint(m.lockoutCount)
		if backoff > maxLockout {
			backoff = maxLockout
		}
		m.lockoutUntil = now.Add(backoff)
		m.lockoutCount++
		// Reset the in-window failure list so the next window starts fresh;
		// the lockout itself prevents attempts until it expires.
		m.failures = m.failures[:0]
	}
}

// resetRateLimit clears rate-limiting state after a successful verification.
// Caller must hold m.mu.
func (m *Manager) resetRateLimit() {
	m.failures = m.failures[:0]
	m.lockoutUntil = time.Time{}
	m.lockoutCount = 0
}

// cleanupSessions removes expired and used pairing sessions from the in-memory
// map and deletes any leftover QR PNG files on disk. It is called on
// CreateSession and on each verify path so the sessions map and orphaned QR
// files cannot grow without bound over the daemon's lifetime. Caller must hold
// m.mu.
func (m *Manager) cleanupSessions() {
	now := time.Now().UTC()
	for id, s := range m.sessions {
		// Remove sessions that have expired or already been consumed (they are
		// single-use). Used sessions already had their QR file removed at
		// issuance, but expired-unused sessions still have one on disk.
		if s.Used || !now.Before(s.ExpiresAt) {
			if s.QRPath != "" {
				// Best-effort removal; a missing file is not an error.
				if err := os.Remove(s.QRPath); err != nil && !os.IsNotExist(err) {
					log.Printf("pairing: failed to remove QR file %s: %v", s.QRPath, err)
				}
			}
			delete(m.sessions, id)
		}
	}
}

// devicesPath returns the on-disk path used to persist device credentials.
func (m *Manager) devicesPath() string {
	return filepath.Join(m.dataDir, devicesFileName)
}

// loadDevices reads persisted device credentials from disk into m.devices. A
// missing file is treated as "no devices yet" (not an error). Any other read
// or parse error is logged loudly and the manager starts with no devices so
// the daemon can still run (previously paired devices must re-pair). Caller
// must NOT hold m.mu (this is only called from NewManager before the manager
// is shared).
func (m *Manager) loadDevices() {
	data, err := os.ReadFile(m.devicesPath()) //nolint:gosec // path is within the app data dir.
	if err != nil {
		if os.IsNotExist(err) {
			return // first run, nothing to load
		}
		log.Printf("pairing: failed to load device credentials from %s: %v", m.devicesPath(), err)
		return
	}

	var records []storedDevice
	if err := json.Unmarshal(data, &records); err != nil {
		// Fail loudly: a corrupt credentials file means prior pairings are
		// unreadable. Start empty rather than silently masking the corruption.
		log.Printf("pairing: failed to parse device credentials from %s: %v", m.devicesPath(), err)
		return
	}

	for i := range records {
		r := records[i] // take address of a stable copy
		m.devices[r.ID] = &r
	}
}

// saveDevices writes the current device credentials to disk. Only the SHA-256
// hashes are persisted (never raw secrets). Caller must hold m.mu.
func (m *Manager) saveDevices() error {
	records := make([]storedDevice, 0, len(m.devices))
	for _, d := range m.devices {
		records = append(records, storedDevice{
			ID:         d.ID,
			Name:       d.Name,
			SecretHash: d.SecretHash,
			PairedAt:   d.PairedAt,
		})
	}

	data, err := json.MarshalIndent(records, "", "  ")
	if err != nil {
		return fmt.Errorf("marshal device credentials: %w", err)
	}

	// Ensure the data directory exists (it normally does, but be safe).
	if err := os.MkdirAll(m.dataDir, 0700); err != nil {
		return fmt.Errorf("create data dir: %w", err)
	}

	if err := os.WriteFile(m.devicesPath(), data, 0600); err != nil {
		return fmt.Errorf("write device credentials: %w", err)
	}
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
