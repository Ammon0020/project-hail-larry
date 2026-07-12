// Package pathutil provides helpers for safely joining filesystem paths
// while preventing path-traversal attacks.
//
// SafeJoin is the shared core used by the files and workspace packages to
// validate that a caller-supplied relative path stays within an allowed root
// directory. It performs lexical checks only; callers that need to defend
// against symlink-based escapes must add their own symlink resolution on top
// (see internal/workspace.safeJoin for an example).
package pathutil

import (
	"fmt"
	"path/filepath"
	"strings"
)

// SafeJoin joins root with relPath, preventing path traversal attacks.
// It rejects absolute paths and paths containing ".." components.
//
// A path is rejected if any individual component equals ".." (a real
// parent-directory traversal) or if the cleaned path is absolute. The final
// containment check (result must stay within root) is the real safety net; the
// component check avoids false-rejecting legitimate filenames such as "..foo"
// that merely begin with the characters "..".
func SafeJoin(root, relPath string) (string, error) {
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
