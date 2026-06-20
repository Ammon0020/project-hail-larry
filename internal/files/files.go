// Package files implements file sync and three-way merge.
// Blueprint references: Sec 14 (File System Access — Client File Sync).
//
// Every file has a monotonic revision number that increments on each write.
// On save, the client sends content plus expectedRevision. If revisions match,
// the host applies and broadcasts. If stale, a three-way merge is attempted.
package files

import (
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

// FileSync implements interfaces.FileSync.
type FileSync struct {
	mu        sync.Mutex
	revisions map[string]int64  // workspaceID/relPath -> current revision
	contents  map[string]string // workspaceID/relPath -> last known content (for three-way merge base)
}

// NewFileSync creates a new FileSync instance.
func NewFileSync() *FileSync {
	return &FileSync{
		revisions: make(map[string]int64),
		contents:  make(map[string]string),
	}
}

// Save writes file content with optimistic locking via expectedRevision.
// Returns the new revision on success. Returns ErrStaleRevision if the file
// has been modified since the client last read it.
//
// In Phase 1, a stale revision returns ErrStaleRevision without attempting
// a three-way merge. The merge UI is handled by the frontend using @codemirror/merge.
func (f *FileSync) Save(ctx context.Context, workspacePath, relPath, content string, expectedRevision int64) (int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()

	key := fileKey(workspacePath, relPath)

	currentRev, exists := f.revisions[key]
	if exists && currentRev != expectedRevision {
		return 0, ErrStaleRevision
	}

	// Write the file to disk.
	fullPath, err := safeJoin(workspacePath, relPath)
	if err != nil {
		return 0, err
	}

	// Ensure parent directory exists.
	dir := filepath.Dir(fullPath)
	if err := os.MkdirAll(dir, 0755); err != nil {
		return 0, fmt.Errorf("create dir: %w", err)
	}

	if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil {
		return 0, fmt.Errorf("write file: %w", err)
	}

	// Increment revision.
	newRev := currentRev + 1
	if !exists {
		newRev = 1
	}
	f.revisions[key] = newRev
	f.contents[key] = content

	return newRev, nil
}

// CurrentRevision returns the latest revision of a file.
// Returns 0 if the file has not been tracked yet.
func (f *FileSync) CurrentRevision(ctx context.Context, workspacePath, relPath string) (int64, error) {
	f.mu.Lock()
	defer f.mu.Unlock()

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
	f.mu.Lock()
	defer f.mu.Unlock()

	key := fileKey(workspacePath, relPath)
	if _, exists := f.revisions[key]; !exists {
		f.revisions[key] = 1
		f.contents[key] = content
	}
}

// GetBaseContent returns the last known content for a file (used as merge base).
func (f *FileSync) GetBaseContent(workspacePath, relPath string) (string, bool) {
	f.mu.Lock()
	defer f.mu.Unlock()

	key := fileKey(workspacePath, relPath)
	content, ok := f.contents[key]
	return content, ok
}

// fileKey generates a unique key for a file within a workspace.
func fileKey(workspacePath, relPath string) string {
	return filepath.Join(workspacePath, relPath)
}

// safeJoin joins a workspace root with a relative path, preventing path traversal.
func safeJoin(root, relPath string) (string, error) {
	cleanRel := filepath.Clean(relPath)
	if strings.HasPrefix(cleanRel, "..") || filepath.IsAbs(cleanRel) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}

	fullPath := filepath.Join(root, cleanRel)

	if !strings.HasPrefix(fullPath, filepath.Clean(root)+string(filepath.Separator)) && fullPath != filepath.Clean(root) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}

	return fullPath, nil
}
