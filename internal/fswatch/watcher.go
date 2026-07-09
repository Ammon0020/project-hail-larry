// Package fswatch watches registered workspace roots for file changes that
// originate OUTSIDE the app (e.g. a file edited in another editor) and emits
// interfaces.EventFileChangedOnDisk for each. The app's own writes are
// suppressed (agent writes already emit EventFileWritten and user saves are
// already reflected in the editor), so this fires only for external changes.
//
// Blueprint references: Sec 14 (File System Access — external change detection).
package fswatch

import (
	"log"
	"os"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/fsnotify/fsnotify"
)

// ignoreDirs are directory names never watched. Mirrors internal/search and the
// file-tree behavior so watch events correspond to what the UI actually shows.
var ignoreDirs = map[string]bool{
	".git":         true,
	"node_modules": true,
	"vendor":       true,
	"dist":         true,
	"build":        true,
	".next":        true,
	"target":       true,
	".cache":       true,
	"coverage":     true,
	"out":          true,
}

const (
	// appWriteSuppression is how long after an app write a matching fsnotify
	// event is ignored (the app's own writes emit their own events elsewhere).
	appWriteSuppression = 2 * time.Second
	// emitThrottle coalesces rapid repeat events for the same path (editors
	// often write a file several times in quick succession during one save).
	emitThrottle = 300 * time.Millisecond
	// cleanupInterval bounds the suppression/throttle bookkeeping maps.
	cleanupInterval = 30 * time.Second
)

// Watcher watches workspace roots for external file changes. It is safe for
// concurrent use by multiple goroutines.
type Watcher struct {
	fsw  *fsnotify.Watcher
	emit func(interfaces.Event)

	mu        sync.Mutex
	roots     map[string]string    // workspaceID -> absolute root path
	appWrites map[string]time.Time // absPath -> time the app wrote it
	lastEmit  map[string]time.Time // absPath -> last emit time (throttle)

	done   chan struct{}
	closed bool // guarded by mu; true once Close has run
	wg     sync.WaitGroup
}

// New creates a Watcher and starts its event loop. emit is invoked for each
// external file change (it may be nil, in which case events are dropped).
// Returns an error only if the underlying fsnotify watcher cannot be created.
func New(emit func(interfaces.Event)) (*Watcher, error) {
	fsw, err := fsnotify.NewWatcher()
	if err != nil {
		return nil, err
	}
	w := &Watcher{
		fsw:       fsw,
		emit:      emit,
		roots:     make(map[string]string),
		appWrites: make(map[string]time.Time),
		lastEmit:  make(map[string]time.Time),
		done:      make(chan struct{}),
	}
	w.wg.Add(1)
	go w.loop()
	return w, nil
}

// AddWorkspace begins watching a workspace root recursively. fsnotify is not
// recursive, so the tree is walked once and every non-ignored directory is
// added; new directories are added on the fly as they are created. Per-path
// errors are logged and skipped so one bad directory doesn't abort the rest.
// If id is already registered with a different absolute path, the old watches
// are removed first so stale fsnotify watches are not leaked. Returns early
// (no-op) if the watcher is closed.
func (w *Watcher) AddWorkspace(id, absPath string) {
	w.mu.Lock()
	if w.closed {
		w.mu.Unlock()
		return
	}
	if existing, ok := w.roots[id]; ok && existing != absPath {
		w.mu.Unlock()
		w.removeWorkspace(id, existing)
		w.mu.Lock()
	}
	w.roots[id] = absPath
	w.mu.Unlock()
	w.addTree(absPath)
}

func (w *Watcher) addTree(root string) {
	_ = filepath.WalkDir(root, func(path string, d os.DirEntry, err error) error {
		if err != nil {
			return nil // skip unreadable entries
		}
		if !d.IsDir() {
			return nil
		}
		name := d.Name()
		if path != root && (ignoreDirs[name] || strings.HasPrefix(name, ".")) {
			return filepath.SkipDir
		}
		if addErr := w.fsw.Add(path); addErr != nil {
			log.Printf("fswatch: add %s: %v", path, addErr)
		}
		return nil
	})
}

// RemoveWorkspace stops watching a workspace root and all its subdirectories.
// Returns early (no-op) if the watcher is closed or id is not registered.
func (w *Watcher) RemoveWorkspace(id string) {
	w.mu.Lock()
	if w.closed {
		w.mu.Unlock()
		return
	}
	root, ok := w.roots[id]
	delete(w.roots, id)
	w.mu.Unlock()
	if !ok {
		return
	}
	w.removeWorkspace(id, root)
}

// removeWorkspace unwatches root and all paths under it. It does not touch
// w.roots; the caller is responsible for the map bookkeeping. The id is
// accepted only for symmetry with RemoveWorkspace (currently unused beyond
// that) and may be passed empty when called from AddWorkspace's re-add path.
func (w *Watcher) removeWorkspace(_, root string) {
	prefix := root + string(os.PathSeparator)
	for _, p := range w.fsw.WatchList() {
		if p == root || strings.HasPrefix(p, prefix) {
			_ = w.fsw.Remove(p)
		}
	}
}

// NoteAppWrite records that the app itself just wrote absPath so the imminent
// fsnotify event for it is suppressed (not surfaced as an external change).
// Returns early (no-op) if the watcher is closed, so shutdown-time hook calls
// don't leak entries into appWrites (whose cleanup loop has exited).
func (w *Watcher) NoteAppWrite(absPath string) {
	w.mu.Lock()
	if w.closed {
		w.mu.Unlock()
		return
	}
	w.appWrites[absPath] = time.Now()
	w.mu.Unlock()
}

// Close stops the watcher's event loop and releases resources. It is
// idempotent: calling it more than once returns nil without panicking.
func (w *Watcher) Close() error {
	w.mu.Lock()
	if w.closed {
		w.mu.Unlock()
		return nil
	}
	w.closed = true
	close(w.done)
	w.mu.Unlock()
	err := w.fsw.Close()
	w.wg.Wait()
	return err
}

func (w *Watcher) loop() {
	defer w.wg.Done()
	ticker := time.NewTicker(cleanupInterval)
	defer ticker.Stop()
	for {
		select {
		case <-w.done:
			return
		case ev, ok := <-w.fsw.Events:
			if !ok {
				return
			}
			w.handle(ev)
		case err, ok := <-w.fsw.Errors:
			if !ok {
				return
			}
			log.Printf("fswatch: %v", err)
		case <-ticker.C:
			w.cleanup()
		}
	}
}

func (w *Watcher) handle(ev fsnotify.Event) {
	// Chmod-only events carry no content/name change — ignore.
	if ev.Op == fsnotify.Chmod {
		return
	}
	path := ev.Name
	base := filepath.Base(path)

	// A newly created directory must be watched too (recursive coverage). The
	// directory creation itself is not a file change to surface.
	if ev.Op&fsnotify.Create != 0 {
		if info, err := os.Stat(path); err == nil && info.IsDir() {
			if !ignoreDirs[base] && !strings.HasPrefix(base, ".") {
				w.addTree(path)
			}
			return
		}
	}

	// Skip anything inside (or named as) an ignored directory.
	if pathHasIgnoredComponent(path) {
		return
	}

	w.mu.Lock()
	// Resolve the owning workspace root.
	var wsID, root string
	for id, r := range w.roots {
		if path == r || strings.HasPrefix(path, r+string(os.PathSeparator)) {
			wsID, root = id, r
			break
		}
	}
	if root == "" {
		w.mu.Unlock()
		return
	}
	// Suppress the app's own writes.
	if t, ok := w.appWrites[path]; ok && time.Since(t) < appWriteSuppression {
		w.mu.Unlock()
		return
	}
	// Throttle repeat events for the same path.
	if t, ok := w.lastEmit[path]; ok && time.Since(t) < emitThrottle {
		w.mu.Unlock()
		return
	}
	w.lastEmit[path] = time.Now()
	emit := w.emit
	w.mu.Unlock()

	rel, err := filepath.Rel(root, path)
	if err != nil {
		return
	}
	rel = filepath.ToSlash(rel)
	if emit != nil {
		emit(interfaces.Event{
			Type:        interfaces.EventFileChangedOnDisk,
			WorkspaceID: wsID,
			Target:      rel,
		})
	}
}

// pathHasIgnoredComponent reports whether any path segment is an ignored dir.
func pathHasIgnoredComponent(path string) bool {
	for _, part := range strings.Split(filepath.ToSlash(path), "/") {
		if ignoreDirs[part] {
			return true
		}
	}
	return false
}

func (w *Watcher) cleanup() {
	now := time.Now()
	w.mu.Lock()
	for p, t := range w.appWrites {
		if now.Sub(t) > appWriteSuppression {
			delete(w.appWrites, p)
		}
	}
	for p, t := range w.lastEmit {
		if now.Sub(t) > cleanupInterval {
			delete(w.lastEmit, p)
		}
	}
	w.mu.Unlock()
}
