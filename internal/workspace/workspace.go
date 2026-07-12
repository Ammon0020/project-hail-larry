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
	"errors"
	"fmt"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/adama/local-agent/internal/interfaces"
	"github.com/adama/local-agent/internal/pathutil"
	"github.com/adama/local-agent/internal/search"
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

	// Optional lifecycle hooks, set by the daemon to wire the filesystem
	// watcher. They are invoked WITHOUT holding mu (to avoid coupling the
	// watcher's work to the map lock) and may be nil. onWrite is called with
	// the absolute path of a file the app itself wrote (so the watcher can
	// suppress the corresponding external-change event); onRegister/onRemove
	// track the set of watched workspace roots.
	onWrite    func(absPath string)
	onRegister func(id, absPath string)
	onRemove   func(id string)
}

// NewManager creates a new workspace Manager.
func NewManager() *Manager {
	return &Manager{
		workspaces: make(map[string]string),
	}
}

// SetOnWrite registers a hook invoked with the absolute path of every file the
// app writes via WriteFile. Used by the filesystem watcher to suppress the
// external-change event for the app's own writes. Call before use.
func (m *Manager) SetOnWrite(fn func(absPath string)) {
	m.mu.Lock()
	m.onWrite = fn
	m.mu.Unlock()
}

// SetOnRegister registers a hook invoked when a workspace is registered, with
// its id and absolute path. Used to start watching the workspace's files.
func (m *Manager) SetOnRegister(fn func(id, absPath string)) {
	m.mu.Lock()
	m.onRegister = fn
	m.mu.Unlock()
}

// SetOnRemove registers a hook invoked when a workspace is removed, with its
// id. Used to stop watching the workspace's files.
func (m *Manager) SetOnRemove(fn func(id string)) {
	m.mu.Lock()
	m.onRemove = fn
	m.mu.Unlock()
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
	onRegister := m.onRegister
	m.mu.Unlock()

	// Notify the watcher (if wired) so the new workspace's files are watched.
	// Called outside the lock to avoid coupling the watcher's tree walk to mu.
	if onRegister != nil {
		onRegister(id, absPath)
	}

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
	if _, ok := m.workspaces[id]; !ok {
		m.mu.Unlock()
		return fmt.Errorf("workspace not found: %s", id)
	}
	delete(m.workspaces, id)
	onRemove := m.onRemove
	m.mu.Unlock()

	// Stop watching the workspace's files (if the watcher is wired). Called
	// outside the lock to avoid coupling the watcher's work to mu.
	if onRemove != nil {
		onRemove(id)
	}
	return nil
}

// maxReadFileSize caps the number of bytes ReadFile will read from disk before
// returning the content. Without a cap, a multi-gigabyte file could be loaded
// into memory by os.ReadFile and OOM the daemon. 50 MiB is large enough for any
// realistic source file yet small enough to bound memory use per request.
const maxReadFileSize = 50 * 1024 * 1024

// ReadFile returns the content of a file and its current revision.
// The revision is a content hash (the leading 48 bits of the SHA-256 of the
// file content), used for optimistic locking. Unlike filesystem ModTime, a
// content hash is deterministic with respect to content (not clock-dependent
// or resolution-limited), so two writes within the same mtime tick produce
// distinct revisions whenever the content differs and the optimistic-lock
// check reliably detects concurrent edits.
//
// Only 48 bits are used (rather than the full 64) so the revision fits within
// JavaScript's Number.MAX_SAFE_INTEGER (2^53-1). The frontend parses the
// revision via JSON.parse, which represents all numbers as IEEE-754 doubles
// and loses precision beyond 2^53; a full 64-bit hash would be rounded,
// causing the optimistic-lock check to fail on every save. 48 bits (2^48
// possible values) keeps the collision probability negligible (~2^-48 per
// comparison) while remaining exactly representable as a JS number.
//
// For binary files, isBinary is true and content is empty. The revision is
// still computed (from the sampled bytes) so the optimistic-lock check works
// defensively if a save is ever attempted.
func (m *Manager) ReadFile(_ context.Context, workspaceID, relPath string) (content string, revision int64, isBinary bool, previewable bool, err error) {
	// Copy the workspace path out under the read lock, then perform all disk
	// I/O without holding the mutex.
	m.mu.RLock()
	wsPath, ok := m.workspaces[workspaceID]
	m.mu.RUnlock()
	if !ok {
		return "", 0, false, false, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	// Prevent path traversal outside the workspace.
	fullPath, err := safeJoin(wsPath, relPath)
	if err != nil {
		return "", 0, false, false, err
	}

	// Bound the read size so a multi-GB file cannot OOM the daemon. Lstat is
	// used (not Stat) so that the size check operates on the link entry itself
	// rather than the target — but safeJoin has already rejected symlinks, so
	// in practice the entry is always a regular file or directory. A directory
	// here will fail the subsequent os.ReadFile with a "is a directory" error,
	// which is the desired behavior.
	info, err := os.Lstat(fullPath)
	if err != nil {
		return "", 0, false, false, fmt.Errorf("stat file: %w", err)
	}
	if info.Size() > maxReadFileSize {
		return "", 0, false, false, fmt.Errorf("file too large (max %d bytes, file is %d bytes)", maxReadFileSize, info.Size())
	}

	// Binary-only preview formats (images, PDF, video, audio, binary 3D models,
	// XLSX, EPUB, etc.) cannot be text-edited and go straight to FileViewer.
	// The content is sampled for a revision so optimistic locking works.
	if isBinaryPreviewExt(relPath) {
		sample, serr := readPrefix(fullPath, binarySniffSize)
		if serr != nil {
			return "", 0, false, true, fmt.Errorf("read file: %w", serr)
		}
		return "", contentRevision(sample), true, true, nil
	}

	// Detect binary content before reading the whole file. We sniff the first
	// 512 bytes and check for null bytes (0x00) — the same heuristic git uses.
	// A null byte within the first 512 bytes is a strong signal that the file
	// is not text; reading it as a string would yield garbage and break JSON
	// serialization. For binary files we return empty content but still
	// compute a revision from the sampled bytes so optimistic locking remains
	// defensive if a save is ever attempted.
	sample, serr := readPrefix(fullPath, binarySniffSize)
	if serr != nil {
		return "", 0, false, isTextPreviewExt(relPath), fmt.Errorf("read file: %w", serr)
	}
	if isBinaryBytes(sample) {
		return "", contentRevision(sample), true, isTextPreviewExt(relPath), nil
	}

	data, err := os.ReadFile(fullPath) //nolint:gosec // fullPath is constrained by safeJoin to the registered workspace root.
	if err != nil {
		return "", 0, false, isTextPreviewExt(relPath), fmt.Errorf("read file: %w", err)
	}

	// Revision is a content hash derived from the file bytes. This is
	// independent of filesystem mtime (which is non-monotonic and coarse) so
	// optimistic locking remains correct.
	return string(data), contentRevision(data), false, isTextPreviewExt(relPath), nil
}

// FilePath returns the absolute filesystem path for a file in the workspace,
// after validating path traversal and symlink constraints via safeJoin. Used
// by the raw file serving endpoint (GET /api/workspaces/{id}/raw) to stream
// file bytes directly to the client with proper Content-Type headers —
// needed for PDF, video, audio, and other binary previews that cannot be
// served through the JSON-wrapped ReadFile endpoint.
//
// Unlike ReadFile, no size limit is enforced here: the caller uses
// http.ServeFile which streams from disk and supports range requests,
// so large media files (e.g. videos) can be served without loading them
// entirely into memory. Access is still bounded by auth (requireAuth) and
// path validation (safeJoin + symlink rejection).
func (m *Manager) FilePath(_ context.Context, workspaceID, relPath string) (string, error) {
	m.mu.RLock()
	wsPath, ok := m.workspaces[workspaceID]
	m.mu.RUnlock()
	if !ok {
		return "", fmt.Errorf("workspace not found: %s", workspaceID)
	}

	fullPath, err := safeJoin(wsPath, relPath)
	if err != nil {
		return "", err
	}

	info, err := os.Lstat(fullPath)
	if err != nil {
		return "", fmt.Errorf("stat file: %w", err)
	}
	if info.IsDir() {
		return "", fmt.Errorf("path is a directory, not a file")
	}
	return fullPath, nil
}

// binarySniffSize is the number of leading bytes inspected for binary
// detection. 512 matches http.DetectContentType's sniff window and git's
// heuristic, and is large enough that a stray null byte early in a real
// binary file is virtually always caught.
const binarySniffSize = 512

// readPrefix reads up to n leading bytes of the file at path. It does not
// treat a short read as an error — files smaller than n simply return their
// full contents.
func readPrefix(path string, n int) ([]byte, error) {
	f, err := os.Open(path) //nolint:gosec // path is constrained by safeJoin to the workspace root.
	if err != nil {
		return nil, err
	}
	defer func() { _ = f.Close() }()
	buf := make([]byte, n)
	nread, err := f.Read(buf)
	if err != nil && !errors.Is(err, io.EOF) {
		return nil, err
	}
	return buf[:nread], nil
}

// isBinaryBytes reports whether the given sample looks like binary content.
// The heuristic is git's: any null byte (0x00) in the sample means binary.
// This is more reliable than http.DetectContentType for formats like docx/xlsx
// whose sniffed MIME type starts with "application/" but are nonetheless
// binary, and avoids false-positiving on JSON/XML which are text.
func isBinaryBytes(sample []byte) bool {
	for _, b := range sample {
		if b == 0 {
			return true
		}
	}
	return false
}

// binaryPreviewExts lists extensions for files that are previewed by the
// frontend's FileViewer and cannot be meaningfully text-edited. These are
// always marked as binary in ReadFile so the frontend routes them directly to
// FileViewer — no CodeMirror editor, no "View Raw" toggle.
var binaryPreviewExts = map[string]bool{
	// Images (browser-native + JS-decoded)
	"png": true, "jpg": true, "jpeg": true, "gif": true, "webp": true,
	"bmp": true, "ico": true, "avif": true, "tiff": true, "tif": true,
	"heic": true, "heif": true,
	// Documents
	"pdf": true, "docx": true, "xlsx": true, "epub": true,
	// Video
	"mp4": true, "webm": true, "ogv": true, "mov": true, "mkv": true,
	// Audio
	"mp3": true, "wav": true, "oga": true, "ogg": true, "flac": true,
	"m4a": true, "aac": true, "opus": true,
	// 3D models (binary formats)
	"stl": true, "glb": true, "ply": true,
}

// textPreviewExts lists extensions for files that have a visual preview in
// FileViewer but are also text-editable. These files open in CodeMirror by
// default (with a "Preview" button to switch to the visual viewer). The
// previewable flag is set to true so the frontend knows to show the button.
var textPreviewExts = map[string]bool{
	// Images (text-based, editable)
	"svg": true,
	// 3D models (text/XML-based, editable)
	"obj": true, "gltf": true, "3mf": true, "dae": true, "wrl": true, "vrml": true,
	// Data (text-based, table preview)
	"csv": true,
	// Web (text-based, rendered preview)
	"html": true, "htm": true,
}

// isBinaryPreviewExt reports whether the file has a binary-only preview extension.
func isBinaryPreviewExt(relPath string) bool {
	return binaryPreviewExts[extLower(relPath)]
}

// isTextPreviewExt reports whether the file has a text-based preview extension
// (editable in CodeMirror with an optional visual preview).
func isTextPreviewExt(relPath string) bool {
	return textPreviewExts[extLower(relPath)]
}

// extLower extracts the lowercase file extension from a path, or "" if none.
func extLower(relPath string) string {
	dot := strings.LastIndexByte(relPath, '.')
	if dot < 0 {
		return ""
	}
	return strings.ToLower(relPath[dot+1:])
}

// contentRevision computes a deterministic revision from file content by taking
// the leading 48 bits of the SHA-256 digest as a positive int64. It changes
// whenever the content changes and is stable across reads of identical
// content, making it suitable for optimistic-lock comparisons.
//
// The width is capped at 48 bits (rather than 64) so the value fits within
// JavaScript's Number.MAX_SAFE_INTEGER (2^53-1): the frontend round-trips the
// revision through JSON.parse/JSON.stringify, which use IEEE-754 doubles and
// silently round integers beyond 2^53. A full 64-bit revision would be
// altered by that round trip and never match the backend's recomputed hash,
// breaking every save. 48 bits is well within the safe range and gives a
// collision probability of ~2^-48 per comparison — negligible for
// file-level optimistic locking.
func contentRevision(content []byte) int64 {
	h := sha256.Sum256(content)
	// Take the first 6 bytes (48 bits) and combine into a positive int64.
	// The result is always in [0, 2^48), so it is non-zero for any realistic
	// content (a SHA-256 whose first 6 bytes are all zero is astronomically
	// unlikely) and fits exactly in a JavaScript number.
	return int64(uint64(h[0])<<40 | uint64(h[1])<<32 | uint64(h[2])<<24 | //nolint:gosec // G115: value is only 48 bits (max 2^48-1), well within int64 range; see comment above.
		uint64(h[3])<<16 | uint64(h[4])<<8 | uint64(h[5]))
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

// Search runs a workspace-wide content search (Blueprint Sec 17 — file search).
// It looks up the workspace path under the read lock and delegates to
// internal/search.Search, which uses ripgrep when available and falls back to
// a Go-native walker otherwise. All returned paths are relative to the
// workspace root.
func (m *Manager) Search(ctx context.Context, workspaceID string, pattern string, opts search.Options) ([]search.Result, error) {
	// Copy the workspace path out under the read lock, then release it before
	// the (potentially slow) search walk.
	m.mu.RLock()
	wsPath, ok := m.workspaces[workspaceID]
	m.mu.RUnlock()
	if !ok {
		return nil, fmt.Errorf("workspace not found: %s", workspaceID)
	}

	opts.Pattern = pattern
	return search.Search(ctx, wsPath, opts)
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
			node.Type = interfaces.FileNodeTypeFolder
			children, err := buildFileTree(root, childRelPath, depth+1, nodeCount)
			if err != nil {
				return nil, err
			}
			node.Children = children
		} else {
			node.Type = interfaces.FileNodeTypeFile
		}

		nodes = append(nodes, node)
	}

	// Sort: directories first, then files, both alphabetically.
	sort.Slice(nodes, func(i, j int) bool {
		if nodes[i].Type != nodes[j].Type {
			return nodes[i].Type == interfaces.FileNodeTypeFolder
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
//
// In addition to lexical traversal checks, safeJoin resolves symlinks on the
// joined path (and, when the final path doesn't yet exist, on its parent
// directory) and re-validates that the resolved path still stays within the
// workspace root. This prevents an agent from creating a symlink via an
// approved shell command (e.g. `ln -s /etc/passwd ./passwd`) and then reading
// or writing through it via the API to escape the workspace boundary.
// os.ReadFile and os.WriteFile follow symlinks, so the lexical check alone is
// not sufficient.
func safeJoin(root, relPath string) (string, error) {
	// Delegate the core lexical traversal checks (clean, reject absolute,
	// reject ".." components, containment) to pathutil.SafeJoin. The shared
	// helper returns a generic traversal error; reformat it to the
	// workspace-specific message so callers see consistent diagnostics.
	fullPath, err := pathutil.SafeJoin(root, relPath)
	if err != nil {
		return "", fmt.Errorf("path %q is outside the workspace root %q", relPath, root)
	}

	// Resolve any symlinks on the joined path and re-validate containment.
	// resolveSymlinks returns the lexically-checked fullPath when the path (or
	// its parent, for not-yet-existing write targets) cannot be fully resolved,
	// but only after verifying the resolvable portion is not a symlink chain
	// escaping the workspace.
	resolved, err := resolveSymlinks(root, fullPath)
	if err != nil {
		return "", err
	}
	return resolved, nil
}

// resolveSymlinks resolves symlinks on path and verifies the resolved path
// still stays within the workspace root. It returns the resolved absolute path
// on success.
//
// Behavior:
//   - If the final path exists, filepath.EvalSymlinks is used to fully resolve
//     it. The resolved path is then checked for containment under root. If the
//     final component is itself a symlink (Lstat reports ModeSymlink), the
//     path is rejected outright — agents must not read or write through
//     symlinks they may have created via approved shell commands.
//   - If the final path does not exist (e.g. a write target that hasn't been
//     created yet), EvalSymlinks fails. In that case the parent directory is
//     resolved instead and the (lexical) final component is appended back.
//     The parent is also Lstat-checked to ensure it is not itself a symlink.
//   - If the parent also cannot be resolved, the lexical fullPath is returned
//     as-is — the lexical containment check in safeJoin is the only remaining
//     safeguard, but at that point no on-disk symlink chain exists to follow.
//
// root is the workspace root (already absolute and cleaned); path is the
// absolute, lexically-validated candidate path under root.
func resolveSymlinks(root, path string) (string, error) {
	cleanRoot := filepath.Clean(root)

	// Reject a symlink at the final component outright. Even if EvalSymlinks
	// would resolve it back inside the workspace, allowing reads/writes through
	// a symlink the agent created is the exact escape we are preventing: a
	// `ln -s /etc/passwd ./passwd` followed by a ReadFile("./passwd") must be
	// blocked regardless of where the link points.
	if li, err := os.Lstat(path); err == nil && li.Mode()&os.ModeSymlink != 0 {
		return "", fmt.Errorf("path %q is a symlink; symlinks are not permitted in workspace file access", path)
	}

	// Try to fully resolve the path. EvalSymlinks follows symlinks in every
	// component and returns the canonical absolute path.
	resolved, err := filepath.EvalSymlinks(path)
	if err == nil {
		if !isWithinRoot(cleanRoot, resolved) {
			return "", fmt.Errorf("resolved path %q escapes the workspace root %q", resolved, cleanRoot)
		}
		return resolved, nil
	}

	// EvalSymlinks failed — typically because the final component doesn't exist
	// yet (a write target). Resolve the parent directory instead and re-append
	// the final component, then re-check containment.
	parent := filepath.Dir(path)
	base := filepath.Base(path)

	// If the parent is itself a symlink, reject: an agent could `ln -s /etc
	// ./etc` and then write `./etc/passwd` to escape the workspace.
	if li, perr := os.Lstat(parent); perr == nil && li.Mode()&os.ModeSymlink != 0 {
		return "", fmt.Errorf("parent directory %q is a symlink; symlinks are not permitted in workspace file access", parent)
	}

	resolvedParent, perr := filepath.EvalSymlinks(parent)
	if perr != nil {
		// Neither the path nor its parent can be resolved (e.g. the parent
		// doesn't exist either). Fall back to the lexical path; the lexical
		// containment check in safeJoin remains the safeguard.
		return path, nil
	}

	if !isWithinRoot(cleanRoot, resolvedParent) {
		return "", fmt.Errorf("resolved parent %q escapes the workspace root %q", resolvedParent, cleanRoot)
	}

	// Re-attach the (lexical) final component to the resolved parent. This is
	// safe because the final component is a leaf name that does not yet exist
	// on disk, so it cannot itself be a symlink.
	resolvedPath := filepath.Join(resolvedParent, base)
	if !isWithinRoot(cleanRoot, resolvedPath) {
		return "", fmt.Errorf("resolved path %q escapes the workspace root %q", resolvedPath, cleanRoot)
	}
	return resolvedPath, nil
}

// isWithinRoot reports whether path is equal to root or lives beneath it. Both
// arguments are expected to be absolute and cleaned.
func isWithinRoot(root, path string) bool {
	if path == root {
		return true
	}
	return strings.HasPrefix(path, root+string(filepath.Separator))
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
		// Bound the read size so a multi-GB file cannot OOM the daemon during
		// the revision check (same rationale as ReadFile's cap).
		info, serr := os.Lstat(fullPath)
		if serr != nil {
			return 0, fmt.Errorf("stat file for revision check: %w", serr)
		}
		if info.Size() > maxReadFileSize {
			return 0, fmt.Errorf("file too large (max %d bytes, file is %d bytes)", maxReadFileSize, info.Size())
		}
		current, rerr := os.ReadFile(fullPath) //nolint:gosec // fullPath is constrained by safeJoin.
		if rerr != nil {
			return 0, fmt.Errorf("read file for revision check: %w", rerr)
		}
		currentRev := contentRevision(current)
		if currentRev != expectedRevision {
			return 0, fmt.Errorf("conflict: file has been modified (expected revision %d, current %d)", expectedRevision, currentRev)
		}
	}

	// Notify the watcher that the app itself is about to write this path so it
	// suppresses the resulting fsnotify event (agent writes separately emit
	// EventFileWritten; user saves are already reflected in the editor). The
	// hook MUST run before os.WriteFile: the fsnotify event is delivered to the
	// watcher's loop asynchronously, and recording the app-write timestamp
	// pre-write guarantees the suppression check in handle() sees it. Read the
	// hook under the lock, then call it without holding mu.
	m.mu.RLock()
	onWrite := m.onWrite
	m.mu.RUnlock()
	if onWrite != nil {
		onWrite(fullPath)
	}

	if err := os.WriteFile(fullPath, []byte(content), 0644); err != nil { //nolint:gosec // fullPath is constrained by safeJoin to the registered workspace root.
		return 0, fmt.Errorf("write file: %w", err)
	}

	// The new revision is the content hash of the written bytes. It is
	// deterministic and reflects the new on-disk content.
	return contentRevision([]byte(content)), nil
}
