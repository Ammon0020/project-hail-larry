# Case-sensitive prefix match breaks watch cleanup on Windows/macOS

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `internal/fswatch/watcher.go`
- **Lines:** 123-128 (also 199)

## Description

`RemoveWorkspace` computes `prefix := root + string(os.PathSeparator)` and removes watches whose paths pass `p == root || strings.HasPrefix(p, prefix)`. The same case-sensitive comparison is used in `handle` (line 199) to resolve the owning workspace. On Windows and on macOS APFS with case-insensitive volumes, fsnotify may report paths whose casing differs from the registered root (e.g., root registered as `C:\Users\Foo\Project`, event path `C:\users\foo\project\file.txt`). The prefix test then fails, so (a) watches are never removed (leak until process exit) and (b) events for that workspace are dropped because `root == ""` in `handle`. AGENTS.md explicitly lists Windows + Mac + Linux as supported platforms.

## Recommendation

Normalize both sides with `filepath.Clean` and compare case-insensitively on Windows/macOS (use `strings.EqualFold` gated on `runtime.GOOS == "windows"` or a case-insensitivity probe), or store roots canonicalized and canonicalize incoming paths the same way before comparison.

## Verification

Read `watcher.go` lines 123-128 and 199 — both use raw `strings.HasPrefix` on `root + os.PathSeparator`. No `filepath.Clean` or case normalization. AGENTS.md line 7 states "Cross platform (Windows, Mac, and Linux)."
