# Search walk has no file/directory count cap (DoS via huge workspace)

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/search/native.rs`
- **Lines:** 49-143 (walker), `src/search/rg.rs:61-169`

## Description

The native fallback walker (`search_with_walker`) iterates the entire `ignore`-crate walk with no limit on the number of files or directories visited. `max_results` (native.rs:79, 125-128) caps only **matches**, not **files scanned**. A workspace containing millions of small files (or a single huge tree the user registered by mistake) forces the blocking task (native.rs:86 `spawn_blocking`) to enumerate every non-ignored file, open + read 512 bytes for binary sniff (native.rs:161-168), and scan line-by-line. Cancellation exists (native.rs:89, 175) but only fires if the client cancels; there is no server-side timeout or file-count cap. The `rg` path is faster but also has no `--max-filecount`/timeout — `--max-count` (rg.rs:72-73) caps matches, not files scanned. Combined with the remote-workspace-arbitrary-path finding, a remote device can register `/` and trigger a search that walks the entire filesystem. The file_tree path has `MAX_FILE_TREE_NODES = 100_000` and `MAX_FILE_TREE_DEPTH = 20` (workspace/mod.rs:20-21) — search has no equivalent.

## Recommendation

Add a `MAX_SEARCH_FILES_SCANNED` cap (e.g. 200k) and a `MAX_SEARCH_WALK_DEPTH` to the native walker, breaking out with a partial result or a `SearchError` when exceeded. For the rg path, add `--max-filecount` (recent rg) or wrap the call in a `tokio::time::timeout` and kill the child on expiry. Apply a server-side deadline in `WorkspaceManager::search` (workspace/mod.rs:300-311) regardless of client cancellation — currently it constructs a fresh `CancellationToken::new()` that is never cancelled by anything.

## Verification

native.rs:86-138 — the loop breaks only on `cancel_clone.is_cancelled()` (line 89) or `results.len() >= max` (lines 126-128, 135-137); there is no `files_scanned` counter. Grep of `src/search` for `max_files|MAX_FILES|file_count|visited|seen` returns no matches. workspace/mod.rs:308 `CancellationToken::new()` is created locally and dropped at function end — nothing cancels it, so the only cancellation source is the client disconnecting (and even that is not wired here).
