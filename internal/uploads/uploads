package uploads

import (
	"bytes"
	"os"
	"path/filepath"
	"testing"
)

// minimalPNG is a valid 1x1 PNG header + IHDR chunk — enough for magic-byte
// detection without being a fully decodable image.
var minimalPNG = append([]byte("\x89PNG\r\n\x1a\n"), make([]byte, 32)...)

func TestStoreAndDetect(t *testing.T) {
	dir := t.TempDir()
	m, err := New(dir)
	if err != nil {
		t.Fatalf("New: %v", err)
	}

	stored, err := m.Store("sess-1", "photo.PNG", bytes.NewReader(minimalPNG))
	if err != nil {
		t.Fatalf("Store: %v", err)
	}
	if stored.MimeType != "image/png" {
		t.Errorf("mimeType = %q, want image/png", stored.MimeType)
	}
	if stored.ID == "" || len(stored.ID) != 32 {
		t.Errorf("id = %q, want 32 hex chars", stored.ID)
	}
	if !filepath.IsAbs(stored.Path) {
		t.Errorf("path %q is not absolute", stored.Path)
	}
	if stored.URI != "file://"+stored.Path {
		t.Errorf("uri = %q, want file://%s", stored.URI, stored.Path)
	}
	// The display name keeps the original extension but the on-disk file uses
	// the upload ID with the magic-byte-derived extension.
	if stored.Name != "photo.PNG" {
		t.Errorf("name = %q, want photo.PNG", stored.Name)
	}
	if filepath.Ext(stored.Path) != ".png" {
		t.Errorf("on-disk ext = %q, want .png", filepath.Ext(stored.Path))
	}
	if _, err := os.Stat(stored.Path); err != nil {
		t.Errorf("stored file missing: %v", err)
	}
}

func TestStoreRejectsUnsupported(t *testing.T) {
	m, _ := New(t.TempDir())
	_, err := m.Store("s", "f.txt", bytes.NewReader([]byte("hello world")))
	if err == nil {
		t.Fatal("expected error for non-image, got nil")
	}
}

func TestStoreRejectsOversize(t *testing.T) {
	m, _ := New(t.TempDir())
	// Build a fake "PNG" that exceeds the limit.
	big := append([]byte("\x89PNG\r\n\x1a\n"), make([]byte, MaxUploadBytes+10)...)
	_, err := m.Store("s", "big.png", bytes.NewReader(big))
	if err == nil {
		t.Fatal("expected oversize error, got nil")
	}
}

func TestGetRoundTrip(t *testing.T) {
	m, _ := New(t.TempDir())
	stored, _ := m.Store("sess", "x.png", bytes.NewReader(minimalPNG))
	got, err := m.Get("sess", stored.ID)
	if err != nil {
		t.Fatalf("Get: %v", err)
	}
	if got != stored.Path {
		t.Errorf("Get = %q, want %q", got, stored.Path)
	}
}

func TestGetRejectsBadID(t *testing.T) {
	m, _ := New(t.TempDir())
	if _, err := m.Get("s", "../escape"); err == nil {
		t.Fatal("expected error for path-traversal id, got nil")
	}
	if _, err := m.Get("s", "tooshort"); err == nil {
		t.Fatal("expected error for short id, got nil")
	}
}

func TestRemoveSession(t *testing.T) {
	m, _ := New(t.TempDir())
	m.Store("s", "x.png", bytes.NewReader(minimalPNG))
	dir := filepath.Join(m.Root(), "s")
	if _, err := os.Stat(dir); err != nil {
		t.Fatalf("session dir missing before remove: %v", err)
	}
	if err := m.RemoveSession("s"); err != nil {
		t.Fatalf("RemoveSession: %v", err)
	}
	if _, err := os.Stat(dir); !os.IsNotExist(err) {
		t.Fatalf("session dir still exists after remove: %v", err)
	}
	// Removing a session with no uploads is a no-op (not an error).
	if err := m.RemoveSession("never-existed"); err != nil {
		t.Errorf("RemoveSession on missing dir: %v", err)
	}
}
