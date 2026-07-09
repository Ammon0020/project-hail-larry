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

	// Match production ordering (workspace.Manager.WriteFile): the app-write
	// timestamp is recorded BEFORE the write so the suppression check in
	// handle() sees it before the fsnotify event is processed.
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

// TestRecursiveCreateDirectoryWatches verifies that a directory created AFTER
// AddWorkspace is recursively watched: a write to a file inside the freshly
// created nested directory must surface as a FileChangedOnDisk event. This
// exercises the Create-directory → addTree path in handle().
func TestRecursiveCreateDirectoryWatches(t *testing.T) {
	dir := t.TempDir()
	w, ch := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	time.Sleep(100 * time.Millisecond)

	nested := filepath.Join(dir, "src", "routes")
	if err := os.MkdirAll(nested, 0755); err != nil {
		t.Fatalf("mkdir: %v", err)
	}
	// Give the Create event + addTree time to register the new directory.
	time.Sleep(150 * time.Millisecond)

	target := filepath.Join(nested, "index.js")
	if err := os.WriteFile(target, []byte("module.exports = 1;"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}

	e, ok := waitForEvent(ch, 3*time.Second)
	if !ok {
		t.Fatal("expected a FileChangedOnDisk event for a file inside a newly created nested dir, got none")
	}
	if e.Type != interfaces.EventFileChangedOnDisk {
		t.Errorf("type = %q, want %q", e.Type, interfaces.EventFileChangedOnDisk)
	}
	want := filepath.ToSlash(filepath.Join("src", "routes", "index.js"))
	if e.Target != want {
		t.Errorf("target = %q, want %q", e.Target, want)
	}
}

// TestEmitThrottleCoalesces verifies that two rapid writes to the same file
// produce exactly one FileChangedOnDisk event (the second is coalesced within
// emitThrottle). We don't rely on the exact throttle window; we only assert
// that no second event arrives within a window comfortably longer than
// emitThrottle but short enough to keep the test fast.
func TestEmitThrottleCoalesces(t *testing.T) {
	dir := t.TempDir()
	w, ch := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	time.Sleep(100 * time.Millisecond)

	p := filepath.Join(dir, "throttle.txt")
	if err := os.WriteFile(p, []byte("a"), 0644); err != nil {
		t.Fatalf("write 1: %v", err)
	}

	e, ok := waitForEvent(ch, 3*time.Second)
	if !ok {
		t.Fatal("expected the first FileChangedOnDisk event, got none")
	}
	if e.Target != "throttle.txt" {
		t.Errorf("first event target = %q, want throttle.txt", e.Target)
	}

	// Second write immediately after the first should fall inside emitThrottle
	// (300ms) and be coalesced — no second event.
	if err := os.WriteFile(p, []byte("bb"), 0644); err != nil {
		t.Fatalf("write 2: %v", err)
	}
	// Wait long enough to detect a spurious second event if one were emitted,
	// but not so long that the test drags. emitThrottle is 300ms; 800ms gives
	// the watcher ample time to deliver any non-coalesced event.
	if e2, ok := waitForEvent(ch, 800*time.Millisecond); ok {
		t.Fatalf("expected exactly one event (coalesced), got a second for %q", e2.Target)
	}
}

// TestDoubleCloseIsSafe verifies that Close is idempotent: calling it twice
// must not panic (no "close of closed channel") and the second call returns
// nil.
func TestDoubleCloseIsSafe(t *testing.T) {
	dir := t.TempDir()
	w, _ := newTestWatcher(t)
	w.AddWorkspace("ws1", dir)
	time.Sleep(50 * time.Millisecond)

	if err := w.Close(); err != nil {
		t.Fatalf("first Close: %v", err)
	}
	// The t.Cleanup in newTestWatcher also calls Close; this second explicit
	// call (plus the cleanup) exercises idempotency.
	if err := w.Close(); err != nil {
		t.Fatalf("second Close: %v", err)
	}
}

// TestEventsStopAfterClose verifies that no FileChangedOnDisk events are
// emitted for writes that happen after Close. The watcher's loop has exited
// and its fsnotify watcher is closed, so any post-close write cannot produce
// an event.
func TestEventsStopAfterClose(t *testing.T) {
	dir := t.TempDir()
	// Build the watcher without newTestWatcher's auto-Cleanup so we control
	// Close timing explicitly.
	ch := make(chan interfaces.Event, 16)
	w, err := New(func(e interfaces.Event) { ch <- e })
	if err != nil {
		t.Fatalf("New: %v", err)
	}
	w.AddWorkspace("ws1", dir)
	time.Sleep(100 * time.Millisecond)

	if err := w.Close(); err != nil {
		t.Fatalf("Close: %v", err)
	}
	// Drain any events already queued from the initial watch setup / close.
	for {
		select {
		case <-ch:
			continue
		default:
		}
		break
	}

	if err := os.WriteFile(filepath.Join(dir, "post-close.txt"), []byte("x"), 0644); err != nil {
		t.Fatalf("write: %v", err)
	}
	if e, ok := waitForEvent(ch, 700*time.Millisecond); ok {
		t.Fatalf("expected no events after Close, got %q", e.Target)
	}
}
