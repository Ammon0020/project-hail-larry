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

	"github.com/adama/local-agent/internal/interfaces"
)

// Manager implements interfaces.WorkspaceManager.
type Manager struct {
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
func (m *Manager) Register(ctx context.Context, path string) (interfaces.WorkspaceInfo, error) {
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

	m.workspaces[id] = absPath

	name := filepath.Base(absPath)

	return interfaces.WorkspaceInfo{
		ID:   id,
		Path: absPath,
		Name: name,
	}, nil
}

// List returns all registered workspaces.
func (m *Manager) List(ctx context.Context) ([]interfaces.WorkspaceInfo, error) {
	workspaces := make([]interfaces.WorkspaceInfo, 0, len(m.workspaces))
	for id, path := range m.workspaces {
		workspaces = append(workspaces, interfaces.WorkspaceInfo{
			ID:   id,
			Path: path,
			Name: filepath.Base(path),
		})
	}
	// Sort by name for stable output.
	sort.Slice(workspaces, func(i, j int) bool {
		return workspaces[i].Name < workspaces[j].Name
	})
	return workspaces, nil
}

// FileTree returns the file tree for a workspace.
// Directories are listed first, then files, both alphabetically.
// Hidden files/directories (starting with .) are excluded.
func (m *Manager) FileTree(ctx context.Context, workspaceID string) ([]interfaces.FileNode, error) {
	path, ok := m.workspaces[workspaceID]
	if !ok {
		return nil, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	return buildFileTree(path, "")
}

// ReadFile returns the content of a file and its current revision.
// The revision is a hash of the file content, used for optimistic locking.
func (m *Manager) ReadFile(ctx context.Context, workspaceID, relPath string) (string, int64, error) {
	wsPath, ok := m.workspaces[workspaceID]
	if !ok {
		return "", 0, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	// Prevent path traversal outside the workspace.
	fullPath, err := safeJoin(wsPath, relPath)
	if err != nil {
		return "", 0, err
	}

	content, err := os.ReadFile(fullPath)
	if err != nil {
		return "", 0, fmt.Errorf("read file: %w", err)
	}

	// Revision is the file size as a simple monotonic-ish counter.
	// In production, this would be a proper revision number tracked by the file-sync package.
	info, err := os.Stat(fullPath)
	if err != nil {
		return "", 0, fmt.Errorf("stat file: %w", err)
	}

	return string(content), info.ModTime().UnixNano(), nil
}

// buildFileTree recursively builds a FileNode tree from the directory at root.
// relPath is the path relative to the workspace root for the current level.
func buildFileTree(root, relPath string) ([]interfaces.FileNode, error) {
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

		childRelPath := filepath.Join(relPath, entry.Name())
		node := interfaces.FileNode{
			Name: entry.Name(),
			Path: childRelPath,
		}

		if entry.IsDir() {
			node.Type = "folder"
			children, err := buildFileTree(root, childRelPath)
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

// safeJoin joins a workspace root with a relative path, preventing path traversal.
func safeJoin(root, relPath string) (string, error) {
	// Clean the relative path to remove any ../ components.
	cleanRel := filepath.Clean(relPath)
	if strings.HasPrefix(cleanRel, "..") || filepath.IsAbs(cleanRel) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}

	fullPath := filepath.Join(root, cleanRel)

	// Verify the result is still within the workspace root.
	if !strings.HasPrefix(fullPath, filepath.Clean(root)+string(filepath.Separator)) && fullPath != filepath.Clean(root) {
		return "", fmt.Errorf("path traversal detected: %s", relPath)
	}

	return fullPath, nil
}
