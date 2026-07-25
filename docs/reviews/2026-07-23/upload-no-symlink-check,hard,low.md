# Upload store does not verify session dir / stored file are not symlinks

- **Difficulty:** hard
- **Urgency:** low
- **File:** `src/uploads/mod.rs`
- **Lines:** 172-188 (`store`), 214-241 (`get`)

## Description

The `store` method calls `self.root.join(session_id)` and then `fs::create_dir_all(&session_dir)` + `fs::write(&abs_path, &data)`. Neither operation checks whether `session_dir` or `abs_path` is a symlink. If a local process running as the same UID (e.g., a malicious ACP agent subprocess, which per the module doc "run as subprocesses on the same host and have filesystem access") plants a symlink at `root/<session_id>` pointing to an arbitrary directory, `create_dir_all` succeeds (no-op on existing target), and `fs::write` follows the symlink — writing the uploaded image to an arbitrary location on disk. Conversely, in `get` (lines 214-241), `entry.metadata()` follows symlinks (it's `fs::metadata`, not `symlink_metadata`), and `tokio::fs::read(&path)` in `serve_upload` (session_extra.rs:209) also follows symlinks. A symlink named `<upload_id>.png` → `/etc/passwd` would cause `serve_upload` to serve the target file's contents to the browser. The codebase has `pathutil::resolve_symlink` and `pathutil::clean_path` used by the workspace manager, but the uploads module does not use them. AGENTS.md mandates "Reject workspace symlinks; contain and validate paths." This risk is mitigated by: (a) 0o700 permissions on the upload root and session dirs, (b) random 32-char hex upload IDs that can't be predicted for pre-planting, and (c) the same-UID threat model where the agent already has filesystem access. It is primarily a defense-in-depth and containment gap.

## Recommendation

After `create_dir_all`, verify `session_dir` via `fs::symlink_metadata` and reject if it's a symlink. In `get`, use `symlink_metadata` on the matched entry and skip symlinks. Alternatively, canonicalize `session_dir` and verify the canonical path is still within `root`.

## Verification

`store` lines 173-183: `session_dir = self.root.join(session_id)`, `create_dir_all`, `abs_path = session_dir.join(&stored_name)`, `fs::write` — no symlink check anywhere. `get` lines 228-234: `entry.metadata()` (follows symlinks) then `entry.path()` returned. `grep` for `symlink_metadata` in `src/uploads/mod.rs` returns zero matches.
