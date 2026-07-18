# Deferred: workspace mutations / browse-preview compact pass (2026-07-18)

Quick maintainability pass applied safe frontend dedup only. Larger/riskier
items left for a dedicated change:

## Deferred (do not drive-by)

1. **Workspace mutation spawn_blocking boilerplate** (`delete_path` /
   `rename_path` / `mkdir` in `src/workspace/mod.rs` + matching
   `WorkspaceManager` methods). Same pattern as `write_file`; extracting a
   shared helper touches async error mapping and fswatch `notify_write` —
   worth a focused PR with tests, not a compact pass.

2. **Prompt-based New File / New Folder UX** (`window.prompt` in `App.tsx`).
   Replacing with an inline tree create row would be a product change, not
   compaction. Client does not strip `../` from prompted names; backend
   `safe_path` / `clean_path` still reject traversal — keep that guarantee.

3. **Browse-preview live reload scope** (`BrowsePreview.tsx`): MVP reloads on
   any `FileWritten` / `FileChangedOnDisk` for the workspace. Path-scoped
   reload (only entry + referenced assets) needs dependency tracking — defer.

4. **Credential-in-query for `/preview/...` and `/raw`**: required for iframe /
   media tags (no Authorization header). Same pattern as existing raw URLs;
   Referer leakage is a known LAN-threat-model tradeoff, not introduced here.
   Hardening (short-lived preview tokens, cookie auth) is a separate design.

## Not must-fix

Path containment for preview/raw/delete/rename/mkdir goes through
`Manager::safe_path` → `clean_path` + `resolve_symlink`. SPA fallback rejects
`/preview/` so traversal that normalizes outside the route cannot fall through
to the IDE shell. No must-fix security issue found in this skim.
