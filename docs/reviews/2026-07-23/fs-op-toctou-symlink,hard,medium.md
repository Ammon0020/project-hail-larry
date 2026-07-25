# TOCTOU between resolve_symlink and the actual fs operation (symlink planted in the window)

- **Difficulty:** hard
- **Urgency:** medium
- **File:** `src/workspace/mod.rs`, `src/files/mod.rs`
- **Lines:** workspace/mod.rs 367-392 (write_file), 396-428 (delete_path), 432-466 (rename_path), 470-484 (mkdir_path); files/mod.rs 219-238 (save_inner)

## Description

`Manager::safe_path` runs `clean_path` + `resolve_symlink` and returns a canonical path that is verified to be inside the root and not a symlink at the leaf. The actual filesystem mutation (`fs::write`, `fs::read`, `fs::remove_file`, `fs::rename`, `fs::create_dir_all`) happens later, using `std::fs`/`tokio::fs` calls that open with `O_CREAT|O_TRUNC` and **follow symlinks** (no `O_NOFOLLOW`). In `write_file` (workspace/mod.rs 373-389) the gap is especially wide: `safe_path` → `symlink_metadata` → `fs::read` (full file read for revision check) → `fs::write`. An attacker with concurrent filesystem access (an approved ACP shell command, or another paired device) can `ln -s /etc/passwd <canonical-path>` in that window; the subsequent `fs::write` follows the new symlink and overwrites the outside target. `resolve_symlink` does not re-check after the operation, and no `O_NOFOLLOW`/`openat2`-style resistant open is used.

## Recommendation

Open write targets with `open(..., O_WRONLY|O_CREAT|O_TRUNC|O_NOFOLLOW)` (or `OpenOptions` with a no-follow flag on platforms that expose it) and reject if the opened fd is a symlink (`fstat` + `S_ISLNK`). For reads/removes, use `openat` with `O_NOFOLLOW` on the final component and `fstat` to confirm a regular file/dir. Re-run `resolve_symlink` immediately before the mutation under the per-file lock, and operate on the opened fd rather than re-resolving the path. On Linux, `openat2` with `RESOLVE_BENEATH | RESOLVE_NO_SYMLINKS` is the robust fix.

## Verification

`write_file` (workspace/mod.rs 373) calls `Manager::safe_path` then `fs::symlink_metadata` (375), `fs::read` (384), and `fs::write(&path, content)` (389) — all on the originally-resolved `path`, with no re-check and no `O_NOFOLLOW`. `delete_path` (418/421) and `rename_path` (460) similarly call `fs::remove_*`/`fs::rename` on the resolved path. `files/mod.rs::save_inner` (221-233) does `clean_path` → `resolve_symlink` → `create_dir_all` → `fs::write` with the same gap. `std::fs::write` uses `OpenOptions::write().create(true).truncate(true)` which does not set `O_NOFOLLOW`.
