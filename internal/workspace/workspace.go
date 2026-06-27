// Package workspace implements workspace management.
// Blueprint references: Sec 13 (Workspace Management), Sec 14 (File System Access).
//
// Workspaces are registered directories on the host. The daemon owns all file
// access within workspace boundaries. This package provides file tree listing,
// file reading, and workspace registration.
package workspace

import (
	"context"
	"crypto/sha256"
	"encoding/hex"
	"fmt"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/adama/local-agent/internal/interfaces"
)

// Manager implements interfaces.WorkspaceManager.
//
// The workspaces map is accessed from concurrent HTTP handlers (registration,
// file tree, git info, lookup by ID, etc.), so every read and write is guarded
// by mu. To avoid holding the lock during slow disk I/O (file tree walks, git
// commands, file reads/writes), methods copy the needed data (the workspace
// path) out under the lock and perform I/O after releasing it.
type Manager struct {
	mu         sync.RWMutex
	workspaces map[string]string // id -> path
}

// NewManager creates a new workspace Manager.
func NewManager() *Manager {
	return &Manager{
		workspaces: make(map[string]string),
	}
}

// Register adds a directory as a workspace.
// Returns the workspace info with a generated ID.
func (m *Manager) Register(_ context.Context, path string) (interfaces.WorkspaceInfo, error) {
	absPath, err := filepath.Abs(path)
	if err != nil {
		return interfaces.WorkspaceInfo{}, fmt.Errorf("abs path: %w", err)
	}

	// Verify the directory exists.
	info, err := os.Stat(absPath)
	if err != nil {
		return interfaces.WorkspaceInfo{}, fmt.Errorf("stat path: %w", err)
	}
	if !info.IsDir() {
		return interfaces.WorkspaceInfo{}, fmt.Errorf("not a directory: %s", absPath)
	}

	// Generate a deterministic ID from the path hash.
	h := sha256.Sum256([]byte(absPath))
	id := hex.EncodeToString(h[:])[:16]

	// All disk I/O is done before acquiring the lock; only the map write is
	// guarded to avoid holding the mutex during slow filesystem operations.
	m.mu.Lock()
	m.workspaces[id] = absPath
	m.mu.Unlock()

	name := filepath.Base(absPath)

	return interfaces.WorkspaceInfo{
		ID:   id,
		Path: absPath,
		Name: name,
	}, nil
}

// List returns all registered workspaces.
func (m *Manager) List(_ context.Context) ([]interfaces.WorkspaceInfo, error) {
	// Snapshot the map under the read lock, then release it before sorting so
	// we never hold the lock longer than necessary.
	m.mu.RLock()
	workspaces := make([]interfaces.WorkspaceInfo, 0, len(m.workspaces))
	for id, path := range m.workspaces {
		workspaces = append(workspaces, interfaces.WorkspaceInfo{
			ID:   id,
			Path: path,
			Name: filepath.Base(path),
		})
	}
	m.mu.RUnlock()

	// Sort by name for stable output.
	sort.Slice(workspaces, func(i, j int) bool {
		return workspaces[i].Name < workspaces[j].Name
	})
	return workspaces, nil
}

// Remove deletes a workspace from the in-memory registry by ID.
// It returns an error if no workspace with the given ID is registered.
// This does not delete any files on disk — only the registration.
func (m *Manager) Remove(_ context.Context, id string) error {
	// The existence check and delete must be atomic under the write lock to
	// avoid a TOCTOU race where another goroutine removes the same ID between
	// the check and the delete.
	m.mu.Lock()
	defer m.mu.Unlock()
	if _, ok := m.workspaces[id]; !ok {
		return fmt.Errorf("workspace not found: %s", id)
	}
	delete(m.workspaces, id)
	return nil
}

// ReadFile returns the content of a file and its current revision.
// The revision is a content hash (the leading 64 bits of the SHA-256 of the
// file content), used for optimistic locking. Unlike filesystem ModTime, a
// content hash is deterministic with respect to content (not clock-dependent
// or resolution-limited), so two writes within the same mtime tick produce
// distinct revisions whenever the content differs and the optimistic-lock
// check reliably detects concurrent edits.
func (m *Manager) ReadFile(_ context.Context, workspaceID, relPath string) (string, int64, error) {
	// Copy the workspace path out under the read lock, then perform all disk
	// I/O without holding the mutex.
	m.mu.RLock()
	wsPath, ok := m.workspaces[workspaceID]
	m.mu.RUnlock()
	if !ok {
		return "", 0, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	// Prevent path traversal outside the workspace.
	fullPath, err := safeJoin(wsPath, relPath)
	if err != nil {
		return "", 0, err
	}

	content, err := os.ReadFile(fullPath) //nolint:gosec // fullPath is constrained by safeJoin to the registered workspace root.
	if err != nil {
		return "", 0, fmt.Errorf("read file: %w", err)
	}

	// Revision is a content hash derived from the file bytes. This is
	// independent of filesystem mtime (which is non-monotonic and coarse) so
	// optimistic locking remains correct.
	return string(content), contentRevision(content), nil
}

// contentRevision computes a deterministic revision from file content by taking
// the leading 64 bits of the SHA-256 digest as a signed int64. It changes
// whenever the content changes and is stable across reads of identical
// content, making it suitable for optimistic-lock comparisons.
func contentRevision(content []byte) int64 {
	h := sha256.Sum256(content)
	// Convert the first 8 bytes to a signed int64. A hash is never all-zero for
	// realistic content, so this is effectively always non-zero.
	return int64(uint64(h[0])<<56 | uint64(h[1])<<48 | uint64(h[2])<<40 | uint64(h[3])<<32 |
		uint64(h[4])<<24 | uint64(h[5])<<16 | uint64(h[6])<<8 | uint64(h[7]))
}

// maxFileTreeDepth caps how deep buildFileTree recurses. This prevents stack
// overflow on pathologically deep trees and (combined with symlink skipping)
// breaks any cycle that could otherwise cause unbounded recursion.
const maxFileTreeDepth = 20

// maxFileTreeNodes caps the total number of nodes returned by buildFileTree to
// avoid exhausting memory on workspaces with millions of entries.
const maxFileTreeNodes = 100000

// FileTree returns the file tree for a workspace.
// Directories are listed first, then files, both alphabetically.
// Hidden files/directories (starting with .) are excluded. Symlinks are
// skipped to avoid cycles, and recursion is depth-limited.
func (m *Manager) FileTree(_ context.Context, workspaceID string) ([]interfaces.FileNode, error) {
	// Look up the workspace path under the read lock and copy it out, then
	// release the lock before the (potentially slow) recursive directory walk.
	m.mu.RLock()
	path, ok := m.workspaces[workspaceID]
	m.mu.RUnlock()
	if !ok {
		return nil, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	return buildFileTree(path, "", 0, new(int))
}

// buildFileTree recursively builds a FileNode tree from the directory at root.
// relPath is the path relative to the workspace root for the current level.
// depth is the current recursion depth (0 at the workspace root); recursion
// stops at maxFileTreeDepth. nodeCount tracks the total nodes produced so far
// across the whole tree and is capped at maxFileTreeNodes. Symlinked entries
// are skipped to prevent cycles (e.g. a symlink pointing at an ancestor).
func buildFileTree(root, relPath string, depth int, nodeCount *int) ([]interfaces.FileNode, error) {
	// Stop recursing beyond the depth cap. We return no children rather than an
	// error so a deep tree is truncated instead of failing the whole listing.
	if depth >= maxFileTreeDepth {
		return nil, nil
	}

	dirPath := filepath.Join(root, relPath)

	entries, err := os.ReadDir(dirPath)
	if err != nil {
		return nil, fmt.Errorf("read dir: %w", err)
	}

	var nodes []interfaces.FileNode

	for _, entry := range entries {
		// Skip hidden files and directories.
		if strings.HasPrefix(entry.Name(), ".") {
			continue
		}

		// Skip symlinks to avoid cycles (a symlink pointing at an ancestor
		// directory would otherwise cause infinite recursion). entry.IsDir()
		// follows symlinks, so check the entry type explicitly via
		// entry.Type() which reflects Lstat (does not follow).
		if entry.Type()&os.ModeSymlink != 0 {
			continue
		}

		// Cap total nodes to bound memory on pathological trees.
		if *nodeCount >= maxFileTreeNodes {
			break
		}
		*nodeCount++

		childRelPath := filepath.Join(relPath, entry.Name())
		node := interfaces.FileNode{
			Name: entry.Name(),
			Path: childRelPath,
		}

		if entry.IsDir() {
			node.Type = "folder"
			children, err := buildFileTree(root, childRelPath, depth+1, nodeCount)
			if err != nil {
				return nil, err
			}
			node.Children = children
		} else {
			node.Type = "file"
		}

		nodes = append(nodes, node)
	}

	// Sort: directories first, then files, both alphabetically.
	sort.Slice(nodes, func(i, j int) bool {
		if nodes[i].Type != nodes[j].Type {
			return nodes[i].Type == "folder"
		}
		return nodes[i].Name < nodes[j].Name
	})

	return nodes, nil
}

// safeJoin joins a workspace root with a relative path, preventing path
// traversal. A path is rejected if any individual component equals ".." (a
// real parent-directory traversal) or if the cleaned path is absolute. The
// final containment check (result must stay within root) is the real safety
// net; the component check avoids false-rejecting legitimate filenames such as
// "..foo" that merely begin with the characters "..".
func safeJoin(root, relPath string) (string, error) {
	// Clean the relative path to remove any redundant components.
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

	// Verify the result is still within the workspace root.
	if !strings.HasPrefix(fullPath, filepath.Clean(root)+string(filepath.Separator)) && fullPath != filepath.Clean(root) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}

	return fullPath, nil
}

// WriteFile writes content to a file in a workspace with optimistic locking.
// If expectedRevision > 0, the current content's hash revision must match;
// otherwise the write is rejected (conflict). Returns the new revision (the
// content hash of the freshly written bytes). Using a content hash instead of
// ModTime makes the optimistic-lock check deterministic with respect to
// content rather than the filesystem's coarse, non-monotonic mtime.
func (m *Manager) WriteFile(_ context.Context, workspaceID, relPath, content string, expectedRevision int64) (int64, error) {
	// Copy the workspace path out under the read lock, then perform all disk
	// I/O (read-check + write) without holding the mutex. The map is only read
	// here; the file-level optimistic locking is handled by the content-hash
	// comparison below.
	m.mu.RLock()
	wsPath, ok := m.workspaces[workspaceID]
	m.mu.RUnlock()
	if !ok {
		return 0, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	// Prevent path traversal outside the workspace.
	fullPath, err := safeJoin(wsPath, relPath)
	if err != nil {
		return 0, err
	}

	// Optimistic locking: when a revision is expected, recompute the current
	// content hash and compare. The hash reflects the actual on-disk content
	// (not mtime), so a concurrent edit that changed the bytes is detected.
	if expectedRevision > 0 {
		current, rerr := os.ReadFile(fullPath) //nolint:gosec // fullPath is constrained by safeJoin.
		if rerr != nil {
			return 0, fmt.Errorf("read file for revision check: %w", rerr)
		}
		currentRev := contentRevision(current)
		if currentRev != expectedRevision {
			return 0, fmt.Errorf("conflict: file has been modified (expected revision %d, current %d)", expectedRevision, currentRev)
		}
	}

	if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil { //nolint:gosec // fullPath is constrained by safeJoin to the registered workspace root.
		return 0, fmt.Errorf("write file: %w", err)
	}

	// The new revision is the content hash of the written bytes. It is
	// deterministic and reflects the new on-disk content.
	return contentRevision([]byte(content)), nil
}
