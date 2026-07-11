# Non-Atomic and Unsafe File Writing

- **Difficulty:** easy
- **Urgency:** high
- **File:** `/media/adam/extex/projects/project-hail-larry/internal/config/config.go`
- **Lines:** 167-181

## Description

The `internal/config` package writes `config.json` via a direct `os.WriteFile` call under a lock. This operation is non-atomic and non-durable. If the process crashes, the system loses power, or disk space runs out during the write, the file is left truncated or corrupted, rendering the agent configuration unreadable.
Similarly, `internal/acp/store.go` persists session state directly using `os.WriteFile`, and `internal/workspace/workspace.go` uses it to write user files.
While `internal/mcp/config.go` implements a custom `WriteFileAtomic` helper, it lacks proper durability practices like calling `Sync` on the file descriptor before renaming, and syncing the parent directory to ensure metadata is flushed on certain filesystems.

## Recommendation

Standardize all critical configuration, metadata, and state writes using a battle-tested atomic writing library:
- **`github.com/google/renameio/v2`** (or `github.com/natefinch/atomic`)

These libraries write to temporary files on the same filesystem, execute `Sync()` to guarantee durability on disk, close the file, and then call `os.Rename()` to atomically swap the old file with the new one. They also handle OS-specific considerations (e.g. Windows filesystem quirks).

## Verification

Code inspection reveals direct `os.WriteFile` usage in [internal/config/config.go#L178](file:///media/adam/extex/projects/project-hail-larry/internal/config/config.go#L178), and a custom `WriteFileAtomic` helper in [internal/mcp/config.go#L140-L167](file:///media/adam/extex/projects/project-hail-larry/internal/mcp/config.go#L140-L167) that does not perform fsync operations.
