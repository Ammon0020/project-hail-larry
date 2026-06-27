// Package files implements file sync and three-way merge.
// Blueprint references: Sec 14 (File System Access — Client File Sync).
//
// Every file has a monotonic revision number that increments on each write.
// On save, the client sends content plus expectedRevision. If revisions match,
// the host applies and broadcasts. If stale, a three-way merge is attempted.
package files

import (
	"container/list"
	"context"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"sync"
)

// ErrStaleRevision is returned when the expected revision doesn't match
// the current revision, indicating a concurrent modification.
var ErrStaleRevision = fmt.Errorf("stale revision: file has been modified since last read")

// maxContentsEntries bounds the in-memory base-content cache. The daemon is
// long-lived and may touch thousands of files; without a bound this cache
// would grow without limit. Eviction is LRU: the least-recently-used entry is
// dropped when the cache is full.
const maxContentsEntries = 256

// FileSync implements interfaces.FileSync.
//
// Concurrency: a single global mutex held across disk I/O serializes all
// workspaces and is a bottleneck. Instead, FileSync uses a per-file lock
// (keyed by workspaceID/relPath) so concurrent saves to different files do not
// block each other, and a short mapMu that is only held for the brief map
// read/write — never across disk I/O. The base-content cache is a bounded LRU.
type FileSync struct {
	// mapMu guards the revisions map and the contents LRU. It is only held for
	// the duration of a map operation, never across disk I/O.
	mapMu sync.Mutex
	// locks maps each file key to a dedicated mutex serializing the
	// check-write-update sequence for that single file. Different files proceed
	// concurrently. Guarded by mapMu.
	locks     map[string]*sync.Mutex
	revisions map[string]int64 // workspaceID/relPath -> current revision

	// contents is a bounded LRU cache of last-known file content, used as the
	// three-way merge base. It is evicted on access so it cannot grow unbounded.
	contents *lruCache
}

// NewFileSync creates a new FileSync instance.
func NewFileSync() *FileSync {
	return &FileSync{
		locks:     make(map[string]*sync.Mutex),
		revisions: make(map[string]int64),
		contents:  newLRUCache(maxContentsEntries),
	}
}

// lockFor returns the per-file mutex for the given key, creating one on first
// use. The locks map itself is guarded by mapMu; the returned mutex is then
// held by the caller for the duration of the per-file operation (including
// disk I/O), which serializes operations on the same file only.
func (f *FileSync) lockFor(key string) *sync.Mutex {
	f.mapMu.Lock()
	defer f.mapMu.Unlock()
	lk, ok := f.locks[key]
	if !ok {
		lk = &sync.Mutex{}
		f.locks[key] = lk
	}
	return lk
}

// Save writes file content with optimistic locking via expectedRevision.
// Returns the new revision on success. Returns ErrStaleRevision if the file
// has been modified since the client last read it.
//
// In Phase 1, a stale revision returns ErrStaleRevision without attempting
// a three-way merge. The merge UI is handled by the frontend using @codemirror/merge.
//
// The revision check and increment happen under a per-file mutex so the
// optimistic-lock check is atomic per file, while disk I/O for one file does
// not block concurrent saves to a different file.
func (f *FileSync) Save(_ context.Context, workspacePath, relPath, content string, expectedRevision int64) (int64, error) {
	key := fileKey(workspacePath, relPath)
	lk := f.lockFor(key)
	lk.Lock()
	defer lk.Unlock()

	// Read the current revision under the brief map mutex.
	f.mapMu.Lock()
	currentRev, exists := f.revisions[key]
	f.mapMu.Unlock()

	if exists && currentRev != expectedRevision {
		return 0, ErrStaleRevision
	}

	// Write the file to disk. Only the per-file mutex is held here, so an
	// unrelated save to a different file/workspace is not blocked.
	fullPath, err := safeJoin(workspacePath, relPath)
	if err != nil {
		return 0, err
	}

	// Ensure parent directory exists.
	dir := filepath.Dir(fullPath)
	if err := os.MkdirAll(dir, 0755); err != nil { //nolint:gosec // workspace files should use normal project directory permissions.
		return 0, fmt.Errorf("create dir: %w", err)
	}

	if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil { //nolint:gosec // workspace files should remain user-editable by normal tools.
		return 0, fmt.Errorf("write file: %w", err)
	}

	// Increment revision and update the base-content cache under the brief map
	// mutex. The per-file mutex guarantees no concurrent writer for this key
	// raced the check-and-update.
	newRev := currentRev + 1
	if !exists {
		newRev = 1
	}
	f.mapMu.Lock()
	f.revisions[key] = newRev
	f.contents.put(key, content)
	f.mapMu.Unlock()

	return newRev, nil
}

// CurrentRevision returns the latest revision of a file.
// Returns 0 if the file has not been tracked yet.
func (f *FileSync) CurrentRevision(_ context.Context, workspacePath, relPath string) (int64, error) {
	f.mapMu.Lock()
	defer f.mapMu.Unlock()
	key := fileKey(workspacePath, relPath)
	rev, exists := f.revisions[key]
	if !exists {
		return 0, nil
	}
	return rev, nil
}

// TrackFile registers a file in the revision tracker with its initial content.
// Called when a file is first read from disk.
func (f *FileSync) TrackFile(workspacePath, relPath, content string) {
	f.mapMu.Lock()
	defer f.mapMu.Unlock()

	key := fileKey(workspacePath, relPath)
	if _, exists := f.revisions[key]; !exists {
		f.revisions[key] = 1
		f.contents.put(key, content)
	}
}

// GetBaseContent returns the last known content for a file (used as merge base).
// Accessing it marks the entry as most-recently-used so the LRU policy keeps
// actively-merged files resident.
func (f *FileSync) GetBaseContent(workspacePath, relPath string) (string, bool) {
	f.mapMu.Lock()
	defer f.mapMu.Unlock()
	key := fileKey(workspacePath, relPath)
	return f.contents.get(key)
}

// Forget drops the cached base content for a file. Call this when a file is
// closed to release memory for files that no longer need a merge base.
func (f *FileSync) Forget(workspacePath, relPath string) {
	f.mapMu.Lock()
	defer f.mapMu.Unlock()
	key := fileKey(workspacePath, relPath)
	f.contents.remove(key)
}

// fileKey generates a unique key for a file within a workspace.
func fileKey(workspacePath, relPath string) string {
	return filepath.Join(workspacePath, relPath)
}

// safeJoin joins a workspace root with a relative path, preventing path
// traversal. A path is rejected if any individual component equals ".." (a real
// parent-directory traversal) or if the cleaned path is absolute. The final
// containment check (result must stay within root) is the real safety net; the
// component check avoids false-rejecting legitimate filenames such as "..foo"
// that merely begin with the characters "..".
func safeJoin(root, relPath string) (string, error) {
	cleanRel := filepath.Clean(relPath)
	if filepath.IsAbs(cleanRel) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}
	// Reject only a real ".." path component, not a filename like "..foo".
	for _, part := range strings.Split(filepath.ToSlash(cleanRel), "/") {
		if part == ".." {
			return "", fmt.Errorf("path traversal detected: %s", relPath)
		}
	}

	fullPath := filepath.Join(root, cleanRel)

	if !strings.HasPrefix(fullPath, filepath.Clean(root)+string(filepath.Separator)) && fullPath != filepath.Clean(root) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}

	return fullPath, nil
}

// lruCache is a simple bounded LRU cache mapping string keys to string values.
// It uses a doubly-linked list (most-recently-used at the front) plus a map for
// O(1) lookup. It is not safe for concurrent use; callers must guard it.
type lruCache struct {
	cap   int
	m     map[string]*list.Element
	order *list.List // element.Value is *lruEntry
}

type lruEntry struct {
	key, value string
}

// newLRUCache creates an LRU cache holding at most cap entries. If cap <= 0 a
// minimum of 1 is used.
func newLRUCache(cap int) *lruCache {
	if cap < 1 {
		cap = 1
	}
	return &lruCache{
		cap:   cap,
		m:     make(map[string]*list.Element),
		order: list.New(),
	}
}

// get returns the value for key and marks it most-recently-used. The ok result
// reports whether the key was present.
func (c *lruCache) get(key string) (string, bool) {
	el, ok := c.m[key]
	if !ok {
		return "", false
	}
	c.order.MoveToFront(el)
	return el.Value.(*lruEntry).value, true
}

// put inserts or updates key=value, evicting the least-recently-used entry when
// over capacity.
func (c *lruCache) put(key, value string) {
	if el, ok := c.m[key]; ok {
		c.order.MoveToFront(el)
		el.Value.(*lruEntry).value = value
		return
	}
	el := c.order.PushFront(&lruEntry{key: key, value: value})
	c.m[key] = el
	for c.order.Len() > c.cap {
		oldest := c.order.Back()
		if oldest == nil {
			break
		}
		c.order.Remove(oldest)
		delete(c.m, oldest.Value.(*lruEntry).key)
	}
}

// remove drops key from the cache if present.
func (c *lruCache) remove(key string) {
	if el, ok := c.m[key]; ok {
		c.order.Remove(el)
		delete(c.m, key)
	}
}
