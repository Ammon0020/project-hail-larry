# Absolute host paths leaked to clients in error messages (information disclosure)

- **Difficulty:** easy
- **Urgency:** low
- **File:** `src/workspace/mod.rs`, `src/pathutil/mod.rs`, `src/interfaces/error.rs`
- **Lines:** workspace/mod.rs 330-333; pathutil/mod.rs 137-141, 148-152, 172-176, 187-191, 198-202; interfaces/error.rs 199-202, 206

## Description

`read_file` (workspace/mod.rs 330-333) constructs a 404 body as `format!("stat file: lstat {}: no such file or directory", path.display())`, embedding the **absolute canonical** path. `PathError::SymlinkEscapesRoot` and `TraversalAttempted` Display strings (pathutil/mod.rs 33-49) include both the resolved absolute path and the absolute workspace root (e.g. "resolved path /home/secret escapes workspace root /home/user/project"). `map_api_error` (interfaces/error.rs 199-202, 206) forwards `path_err.to_string()` and `msg.clone()` verbatim into the JSON `{"error": ...}` body returned to the browser/ACP client. This leaks the host's filesystem layout (username, absolute project location, sibling directories) to any paired device or agent that probes with traversal-style inputs.

## Recommendation

Map `PathError` and `AppError::NotFound` to generic client-facing strings ("path not found", "path traversal rejected") without the absolute path. Log the full path server-side at debug level only.

## Verification

`app_error` (api/mod.rs 1552-1559) puts `mapped.body.error` directly into `ApiResponseError::message`. `map_api_error` (interfaces/error.rs 201) uses `path_err.to_string()` for `AppError::Path`. `PathError` Display variants (pathutil/mod.rs 33-49) interpolate `{0}` which callers fill with `path.display()` / `resolved.display()` / `workspace_root.display()` (e.g. lines 138-140, 149-151). `read_file`'s NotFound (workspace/mod.rs 330-333) embeds `path.display()`.
