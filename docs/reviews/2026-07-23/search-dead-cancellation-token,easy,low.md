# WorkspaceManager::search cancellation token is never cancellable (dead cancellation)

- **Difficulty:** easy
- **Urgency:** low
- **File:** `src/workspace/mod.rs`
- **Lines:** 300-311

## Description

`search` constructs `CancellationToken::new()` inline and passes it to `crate::search::search`, then drops it. No `CancelHandle` is retained, no timeout is attached, and the API handler (api/mod.rs:865-884) does not pass any cancellation either. The token therefore can never be cancelled, so the search runs to completion regardless of client disconnect or elapsed time. This makes the DoS in the search-no-file-count-cap finding worse (no server-side abort) and means a slow search cannot be stopped by the user closing the tab.

## Recommendation

Either accept the request's disconnect signal (axum handler cancellation) and wire it to a `CancellationToken` passed down, or wrap the search in `tokio::time::timeout` with a configurable deadline and cancel on expiry. At minimum, remove the misleading `CancellationToken::new()` and document that search is uncancellable, or thread a real token from the handler.

## Verification

workspace/mod.rs:308 `crate::search::search(&root, &opts, CancellationToken::new())` — the token is a temporary with no clone retained. api/mod.rs:865-884 `search` handler has no cancellation plumbing. `CancellationToken::new()` returns a token whose `cancel()` would need a shared handle; none exists.
