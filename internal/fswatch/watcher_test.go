package fswatch

import (
	"os"
	"path/filepath"
	"testing"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
)

// waitForEvent waits up to timeout for an event on ch, returning it and true,
// or a zero Event and false on timeout.
func waitForEvent(ch <-chan interfaces.Event, timeout time.Duration) (interfaces.Event, bool) {
	select {
	case e := <-ch:
		return e, true
	case <-time.After(timeout):
		return interfaces.Event{}, false
	}
}

func newTestWatcher(t *testing.T) (*Watcher, <-chan interfaces.Event) {
	t.Helper()
	ch := make(chan interfaces.Event, 16)
	w, err := New(func(e interfaces.Event) { ch <- e })
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	t.Cleanup(func() { _ = w.Close() })
	return w, ch
}

func TestExternalChangeEmitsEvent(t *testing.T) {
	dir := t.TempDir()
	w, ch := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	// Give fsnotify a moment to establish the watch.
	time.Sleep(100 * time.Millisecond)

	if err := os.WriteFile(filepath.Join(dir, "hello.txt"), []byte("hi"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}

	e, ok := waitForEvent(ch, 3*time.Second)
	if !ok {
		t.Fatal("expected a FileChangedOnDisk event, got none")
	}
	if e.Type != interfaces.EventFileChangedOnDisk {
		t.Errorf("type = %q, want %q", e.Type, interfaces.EventFileChangedOnDisk)
	}
	if e.WorkspaceID != "ws1" {
		t.Errorf("workspaceID = %q, want ws1", e.WorkspaceID)
	}
	if e.Target != "hello.txt" {
		t.Errorf("target = %q, want hello.txt", e.Target)
	}
}

func TestAppWriteIsSuppressed(t *testing.T) {
	dir := t.TempDir()
	w, ch := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	time.Sleep(100 * time.Millisecond)

	// Mark the path as an app write immediately before writing it; the watcher
	// must suppress the resulting fsnotify event.
	p := filepath.Join(dir, "app.txt")
	w.NoteAppWrite(p)
	if err := os.WriteFile(p, []byte("hi"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}

	if e, ok := waitForEvent(ch, 700*time.Millisecond); ok {
		t.Fatalf("expected app write to be suppressed, got event for %q", e.Target)
	}
}

func TestIgnoredDirNotWatched(t *testing.T) {
	dir := t.TempDir()
	if err := os.MkdirAll(filepath.Join(dir, "node_modules"), 0755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	w, ch := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	time.Sleep(100 * time.Millisecond)

	if err := os.WriteFile(filepath.Join(dir, "node_modules", "x.js"), []byte("x"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}
	if e, ok := waitForEvent(ch, 700*time.Millisecond); ok {
		t.Fatalf("expected ignored dir to be skipped, got event for %q", e.Target)
	}
}

func TestRemoveWorkspaceStopsEvents(t *testing.T) {
	dir := t.TempDir()
	w, ch := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	time.Sleep(100 * time.Millisecond)
	w.RemoveWorkspace("ws1")
	time.Sleep(50 * time.Millisecond)

	if err := os.WriteFile(filepath.Join(dir, "after.txt"), []byte("x"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}
	if e, ok := waitForEvent(ch, 700*time.Millisecond); ok {
		t.Fatalf("expected no events after RemoveWorkspace, got %q", e.Target)
	}
}
