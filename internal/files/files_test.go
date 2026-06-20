package files

import (
	"context"
	"errors"
	"os"
	"path/filepath"
	"testing"
)

// TestSaveNewFile verifies saving a new file creates it with revision 1.
func TestSaveNewFile(t *testing.T) {
	fs := NewFileSync()
	ctx := context.Background()
	wsDir := t.TempDir()

	rev, err := fs.Save(ctx, wsDir, "test.txt", "hello world", 0)
	if err != nil {
		t.Fatalf("save: %v", err)
	}
	if rev != 1 {
		t.Errorf("expected revision 1, got %d", rev)
	}

	// Verify file was written to disk.
	content, err := os.ReadFile(filepath.Join(wsDir, "test.txt"))
	if err != nil {
		t.Fatalf("read file: %v", err)
	}
	if string(content) != "hello world" {
		t.Errorf("expected 'hello world', got %s", string(content))
	}
}

// TestSaveUpdate verifies that saving with the correct revision increments it.
func TestSaveUpdate(t *testing.T) {
	fs := NewFileSync()
	ctx := context.Background()
	wsDir := t.TempDir()

	// First save.
	rev1, _ := fs.Save(ctx, wsDir, "file.txt", "v1", 0)

	// Second save with correct revision.
	rev2, err := fs.Save(ctx, wsDir, "file.txt", "v2", rev1)
	if err != nil {
		t.Fatalf("save v2: %v", err)
	}
	if rev2 != rev1+1 {
		t.Errorf("expected revision %d, got %d", rev1+1, rev2)
	}

	// Verify content on disk.
	content, _ := os.ReadFile(filepath.Join(wsDir, "file.txt"))
	if string(content) != "v2" {
		t.Errorf("expected 'v2', got %s", string(content))
	}
}

// TestSaveStaleRevision verifies that saving with a stale revision fails.
func TestSaveStaleRevision(t *testing.T) {
	fs := NewFileSync()
	ctx := context.Background()
	wsDir := t.TempDir()

	// First save.
	rev1, _ := fs.Save(ctx, wsDir, "file.txt", "v1", 0)

	// Second save (simulates another device writing).
	fs.Save(ctx, wsDir, "file.txt", "v2-from-other", rev1)

	// Try to save with the old revision — should fail.
	_, err := fs.Save(ctx, wsDir, "file.txt", "v2-from-me", rev1)
	if !errors.Is(err, ErrStaleRevision) {
		t.Errorf("expected ErrStaleRevision, got %v", err)
	}
}

// TestCurrentRevision verifies revision tracking.
func TestCurrentRevision(t *testing.T) {
	fs := NewFileSync()
	ctx := context.Background()
	wsDir := t.TempDir()

	// No file tracked yet.
	rev, err := fs.CurrentRevision(ctx, wsDir, "file.txt")
	if err != nil {
		t.Fatalf("current revision: %v", err)
	}
	if rev != 0 {
		t.Errorf("expected revision 0 for untracked file, got %d", rev)
	}

	// Save and check.
	fs.Save(ctx, wsDir, "file.txt", "content", 0)

	rev, _ = fs.CurrentRevision(ctx, wsDir, "file.txt")
	if rev != 1 {
		t.Errorf("expected revision 1, got %d", rev)
	}
}

// TestTrackFile verifies that tracking sets the initial revision.
func TestTrackFile(t *testing.T) {
	fs := NewFileSync()
	wsDir := t.TempDir()

	fs.TrackFile(wsDir, "existing.txt", "existing content")

	ctx := context.Background()
	rev, _ := fs.CurrentRevision(ctx, wsDir, "existing.txt")
	if rev != 1 {
		t.Errorf("expected revision 1 after tracking, got %d", rev)
	}

	// GetBaseContent should return the tracked content.
	content, ok := fs.GetBaseContent(wsDir, "existing.txt")
	if !ok {
		t.Fatal("expected base content to exist")
	}
	if content != "existing content" {
		t.Errorf("expected 'existing content', got %s", content)
	}
}

// TestSaveNestedPath verifies that saving creates parent directories.
func TestSaveNestedPath(t *testing.T) {
	fs := NewFileSync()
	ctx := context.Background()
	wsDir := t.TempDir()

	_, err := fs.Save(ctx, wsDir, "src/routes/index.js", "console.log('hi');", 0)
	if err != nil {
		t.Fatalf("save nested: %v", err)
	}

	// Verify file exists.
	_, err = os.Stat(filepath.Join(wsDir, "src", "routes", "index.js"))
	if err != nil {
		t.Fatalf("file not created: %v", err)
	}
}

// TestSavePathTraversal verifies that path traversal is blocked.
func TestSavePathTraversal(t *testing.T) {
	fs := NewFileSync()
	ctx := context.Background()
	wsDir := t.TempDir()

	_, err := fs.Save(ctx, wsDir, "../../../etc/passwd", "malicious", 0)
	if err == nil {
		t.Error("expected error for path traversal")
	}
}
