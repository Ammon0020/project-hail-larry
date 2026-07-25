# Workspace root accepted even when it IS a symlink (violates "Reject workspace symlinks")

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/workspace/mod.rs`
- **Lines:** 155-178

## Description

`register` validates the root with `fs::metadata` (which **follows** symlinks) and `canonicalize` (which also follows symlinks). There is no `fs::symlink_metadata` check on the root itself. AGENTS.md mandates "Reject workspace symlinks," but a root that is itself a symlink (e.g. `ln -s /etc ~/etc && local_agent add-folder ~/etc`) is silently accepted and stored as its canonical target (`/etc`). The "reject symlinks" policy is enforced for *contents* (file ops via `resolve_symlink`, file_tree via `file_type.is_symlink()` at line 511) but not for the *root*. A user/agent who can influence the registration path can point the workspace at any directory on the host by pre-creating a symlink.

## Recommendation

In `register`, after `fs::metadata`, call `fs::symlink_metadata(&input)` and reject with `AppError::validation("workspace root must not be a symlink")` if `file_type().is_symlink()`. Also reject if any parent component up to the canonical root is a symlink (or rely on a `symlink_metadata` walk of parents). Keep the `canonicalize` for the stored id, but gate acceptance on the link check first.

## Verification

Lines 158-166 use `fs::metadata(&input)` (follows links) and `input.canonicalize()` (follows links). Grep of `src/workspace/mod.rs` for `symlink_metadata|is_symlink` shows no hit inside `register` (lines 155-178); the 11 hits are all in `read_file`/`write_file`/`delete_path`/`rename_path`/`mkdir_path`/`build_tree` — i.e. content ops, never the root.
