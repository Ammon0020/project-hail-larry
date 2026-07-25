# write_file in files/mod.rs trusts workspace_id as a filesystem path

- **Difficulty:** easy
- **Urgency:** low
- **File:** `src/files/mod.rs`
- **Lines:** 190-266 (save_inner), esp. 221-224

## Description

`FileSync::save_inner` treats the `workspace_id` argument as a filesystem path (`clean_path(Path::new(&workspace_path), &rel_path)`, line 221). The doc comment (lines 181-185) acknowledges this is a placeholder pending S-WORKSPACE wiring. If any caller ever passes a client-controlled `workspace_id` directly to `FileSync::save` instead of resolving it through the workspace registry, the client would effectively choose the workspace root. Today the API layer routes through `state.workspaces.write_file` (workspace Manager), so this is latent; it becomes exploitable the moment `FileSync` is wired without the manager resolving the id.

## Recommendation

Change `FileSync`'s signature to take an already-resolved absolute root, or resolve `workspace_id` through the registry inside `FileSync` before any path operation. Add a debug-assert / type distinction so a raw client string cannot reach `clean_path` as a root.

## Verification

`save_inner` (files/mod.rs 221) calls `clean_path(Path::new(&workspace_path), &rel_path)` where `workspace_path` is the `workspace_id: &str` parameter (line 192). The trait impl `save` (342-351) forwards `workspace_id` unchanged. The comment at 181-185 confirms this is intentional-but-temporary.
