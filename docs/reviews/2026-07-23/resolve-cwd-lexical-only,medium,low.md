# resolve_cwd uses only lexical clean_path, not resolve_symlink

- **Difficulty:** medium
- **Urgency:** low
- **File:** `src/shell/mod.rs`
- **Lines:** 401-419 (`resolve_cwd`)

## Description

`resolve_cwd` calls `clean_path` (lexical: rejects absolute inputs and `..` components) but **not** `resolve_symlink` (on-disk canonicalisation). The `pathutil` module explicitly documents the two-layer defence (pathutil/mod.rs:7-14) and warns that `clean_path` alone is insufficient against symlink escapes — an agent that previously created `ln -s /etc ./etc` (via an approved command) could then pass `cwd="etc"`; `clean_path` accepts it (no `..`, not absolute), `current_dir` follows the symlink, and the process runs in `/etc`. This is currently mitigated for the only production caller because `create_terminal` pre-validates cwd through `terminal_cwd` (core.rs:2503-2531), which does `canonicalize` + `strip_prefix` and returns a relative path that `resolve_cwd` then re-accepts. But the `Executor` API itself does not enforce on-disk containment, so any future caller that passes a raw agent cwd directly to `Executor::run*` is vulnerable.

## Recommendation

Have `resolve_cwd` call `resolve_symlink` on the joined result (or at least `canonicalize` + `is_within_root`) before returning, matching the file-access path. Keep `terminal_cwd` as the user-facing validator but make the executor self-defending.

## Verification

shell/mod.rs:418 — `clean_path(workspace_root, cwd)` is the only check; no `resolve_symlink` call. pathutil/mod.rs:7-14 documents that `clean_path` is "lexical only" and callers "that need to defend against symlink-based escapes must layer `resolve_symlink` on top." `terminal_cwd` (core.rs:2513-2522) does the canonicalisation that `resolve_cwd` skips.
