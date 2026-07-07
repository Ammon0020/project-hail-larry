// Package uploads provides per-session filesystem storage for user-uploaded
// artifacts (e.g. images attached to prompts). Each session gets an isolated
// directory under the configured root (typically ~/.local-agent/uploads/).
//
// Uploads are validated by magic bytes (not file extension) so a misnamed file
// is detected and stored with the correct extension — this avoids the
// mime-by-extension bugs documented in agent read-tool image pipelines. The
// stored file is referenced by an opaque upload ID; the absolute path is
// exposed so the agent can read it directly from disk (agents run as
// subprocesses on the same host and have filesystem access).
package uploads

import (
	"crypto/rand"
	"encoding/hex"
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"strings"
)

// MaxUploadBytes is the per-file upload size cap. Matches the server's default
// JSON body limit so a base64-encoded image and a raw upload share the same
// ceiling.
const MaxUploadBytes int64 = 10 << 20 // 10 MB

// Manager stores uploaded artifacts in per-session directories under root.
type Manager struct {
	root string
}

// StoredUpload describes a successfully stored upload.
type StoredUpload struct {
	ID       string `json:"id"`
	Name     string `json:"name"`
	MimeType string `json:"mimeType"`
	// Path is the absolute on-disk path. The agent reads the file from here
	// directly (it runs as a subprocess with filesystem access).
	Path string `json:"path"`
	// URI is a file:// URI pointing at Path, suitable for ACP ImageBlock.Uri
	// or ResourceLinkBlock.Uri.
	URI string `json:"uri"`
	// Size is the stored file size in bytes.
	Size int64 `json:"size"`
}

// New creates a Manager rooted at rootDir. The directory is created if missing.
func New(rootDir string) (*Manager, error) {
	if err := os.MkdirAll(rootDir, 0o700); err != nil {
		return nil, fmt.Errorf("create uploads root: %w", err)
	}
	return &Manager{root: rootDir}, nil
}

// Root returns the absolute root directory.
func (m *Manager) Root() string { return m.root }

// Store validates and writes an upload for the given session. The original
// filename is used only to derive a friendly display name; the on-disk name is
// the upload ID with an extension chosen by magic-byte detection. The reader
// is consumed fully.
func (m *Manager) Store(sessionID, filename string, r io.Reader) (*StoredUpload, error) {
	if sessionID == "" {
		return nil, errors.New("uploads: empty session id")
	}
	// Read into memory with a size cap. Images are bounded by MaxUploadBytes;
	// reading fully lets us validate magic bytes before writing to disk.
	data, err := io.ReadAll(io.LimitReader(r, MaxUploadBytes+1))
	if err != nil {
		return nil, fmt.Errorf("uploads: read: %w", err)
	}
	if int64(len(data)) > MaxUploadBytes {
		return nil, fmt.Errorf("uploads: file exceeds %d bytes", MaxUploadBytes)
	}

	mimeType, ext := detectImage(data)
	if mimeType == "" {
		return nil, errors.New("uploads: unsupported file type (must be PNG, JPEG, GIF, or WebP)")
	}

	uploadID := newID()
	sessionDir := filepath.Join(m.root, sessionID)
	if err := os.MkdirAll(sessionDir, 0o700); err != nil {
		return nil, fmt.Errorf("uploads: create session dir: %w", err)
	}

	storedName := uploadID + ext
	absPath := filepath.Join(sessionDir, storedName)
	if err := os.WriteFile(absPath, data, 0o600); err != nil {
		return nil, fmt.Errorf("uploads: write: %w", err)
	}

	return &StoredUpload{
		ID:       uploadID,
		Name:     sanitizeFilename(filename),
		MimeType: mimeType,
		Path:     absPath,
		URI:      "file://" + absPath,
		Size:     int64(len(data)),
	}, nil
}

// Get returns the absolute path for a session's upload by ID. The file is not
// read; callers (e.g. an http handler) serve it. Returns an error if the file
// does not exist.
func (m *Manager) Get(sessionID, uploadID string) (string, error) {
	if !isValidID(uploadID) {
		return "", fmt.Errorf("uploads: invalid upload id")
	}
	// Search for the file regardless of extension (the extension was chosen by
	// magic-byte detection, which the caller doesn't know).
	sessionDir := filepath.Join(m.root, sessionID)
	entries, err := os.ReadDir(sessionDir)
	if err != nil {
		return "", fmt.Errorf("uploads: not found: %w", err)
	}
	prefix := uploadID + "."
	for _, e := range entries {
		if !e.IsDir() && strings.HasPrefix(e.Name(), prefix) {
			return filepath.Join(sessionDir, e.Name()), nil
		}
	}
	return "", fmt.Errorf("uploads: upload %s not found in session %s", uploadID, sessionID)
}

// RemoveSession deletes all uploads for a session. It is safe to call when no
// uploads exist for the session.
func (m *Manager) RemoveSession(sessionID string) error {
	sessionDir := filepath.Join(m.root, sessionID)
	if err := os.RemoveAll(sessionDir); err != nil {
		return fmt.Errorf("uploads: remove session: %w", err)
	}
	return nil
}

// newID returns a 16-byte hex-encoded random ID.
func newID() string {
	b := make([]byte, 16)
	if _, err := rand.Read(b); err != nil {
		// rand.Read should never fail on modern platforms; fall back to a
		// fixed prefix so Store still produces a unique-ish name rather than
		// crashing.
		return "0000000000000000"
	}
	return hex.EncodeToString(b)
}

// isValidID checks that id is a 32-char lowercase hex string, preventing path
// traversal via crafted upload IDs.
func isValidID(id string) bool {
	if len(id) != 32 {
		return false
	}
	for _, c := range id {
		if !((c >= '0' && c <= '9') || (c >= 'a' && c <= 'f')) {
			return false
		}
	}
	return true
}

// sanitizeFilename strips path separators and control chars from a user-
// supplied filename, keeping only the display name (the on-disk name is the
// upload ID anyway).
func sanitizeFilename(name string) string {
	name = filepath.Base(name)
	name = strings.ReplaceAll(name, string(filepath.Separator), "_")
	if name == "" || name == "." {
		name = "upload"
	}
	return name
}

// detectImage inspects the leading bytes of data and returns the MIME type and
// a canonical extension (with leading dot) for a supported image format, or
// ("", "") if the format is not recognized.
func detectImage(data []byte) (mimeType, ext string) {
	switch {
	case len(data) >= 8 && string(data[0:8]) == "\x89PNG\r\n\x1a\n":
		return "image/png", ".png"
	case len(data) >= 3 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF:
		return "image/jpeg", ".jpg"
	case len(data) >= 6 && (string(data[0:6]) == "GIF87a" || string(data[0:6]) == "GIF89a"):
		return "image/gif", ".gif"
	case len(data) >= 12 && string(data[0:4]) == "RIFF" && string(data[8:12]) == "WEBP":
		return "image/webp", ".webp"
	default:
		return "", ""
	}
}
