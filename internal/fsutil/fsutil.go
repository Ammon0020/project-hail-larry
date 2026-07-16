// Package fsutil provides shared filesystem helpers used by config, MCP, ACP
// conversation store, and other durable state writers.
//
// Mirrors Rust `src/fsutil`: atomic durable writes live here so packages do
// not re-implement temp+fsync+rename, and so the helper is not owned by the
// MCP package (which only happens to contain the original Go implementation).
package fsutil

import (
	"fmt"
	"os"
	"path/filepath"
)

// AppDataDirPerm is the permission used when creating application state
// directories under ~/.local-agent/ (or LOCAL_AGENT_STATE_DIR). The data dir is
// private to the current user and may contain secrets.
const AppDataDirPerm = 0o700

// WriteFileAtomic writes data to path atomically (temp file in the same
// directory + MkdirAll + Chmod + Sync + rename) so a crashed write never
// leaves a half-written file. The parent directory is created with mode
// [AppDataDirPerm] if missing; the file is written with the given perm (0600
// for mcp.json / config.json / conversation store, which may contain secrets
// or app state). The temp file lives in the same directory so the rename is
// guaranteed to be on the same filesystem. Sync flushes file data and
// metadata before the rename; the parent directory is synced best-effort so
// the directory entry is durable on filesystems that require it.
//
// Used by config, MCP, ACP conversation store, and server raw MCP PUT.
func WriteFileAtomic(path string, data []byte, perm os.FileMode) error {
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, AppDataDirPerm); err != nil {
		return fmt.Errorf("create dir: %w", err)
	}
	// Prefix with the target basename so concurrent writers of different files
	// in the same directory do not share a single temp-name pattern.
	tmp, err := os.CreateTemp(dir, "."+filepath.Base(path)+".*.tmp")
	if err != nil {
		return fmt.Errorf("create temp file: %w", err)
	}
	tmpName := tmp.Name()
	// Best-effort cleanup if any step below fails before the rename succeeds.
	// After a successful rename the temp path is gone; Remove is a no-op then.
	defer func() { _ = os.Remove(tmpName) }()
	if _, err := tmp.Write(data); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("write temp file: %w", err)
	}
	if err := tmp.Chmod(perm); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("chmod temp file: %w", err)
	}
	// Flush contents and metadata to stable storage before rename so a power
	// loss cannot leave a renamed-but-empty/truncated file.
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return fmt.Errorf("sync temp file: %w", err)
	}
	if err := tmp.Close(); err != nil {
		return fmt.Errorf("close temp file: %w", err)
	}
	if err := os.Rename(tmpName, path); err != nil {
		return fmt.Errorf("rename into place: %w", err)
	}
	// Best-effort directory sync so the new dirent is durable. Some platforms
	// (notably Windows) reject Sync on directories; ignore those errors.
	// #nosec G304 -- dir is derived from the path argument (a config/state file
	// path controlled by the application), not user input.
	if d, err := os.Open(dir); err == nil {
		_ = d.Sync()
		_ = d.Close()
	}
	return nil
}
