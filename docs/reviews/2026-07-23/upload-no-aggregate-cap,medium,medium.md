# Unbounded aggregate upload storage per session (disk-fill DoS)

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `src/uploads/mod.rs`
- **Lines:** 143-199 (`store` method)

## Description

Each individual upload is capped at `MAX_UPLOAD_BYTES` (10 MiB) via the `reader.take(MAX_UPLOAD_BYTES + 1)` check (lines 161-165). However, there is **no limit on the total number of uploads or aggregate bytes per session**. The `store` method unconditionally creates the session directory and writes the file. An authenticated remote device (or any loopback process) can repeatedly POST to `/api/sessions/{id}/uploads` with 10 MiB images until the disk is full. There is no rate limiting on the upload endpoint — `require_pair_rate_limit` (line 1329 of `api/mod.rs`) only guards pairing endpoints, not upload routes. Cleanup only happens when `close_session` is called (lines 1148-1155 of `api/mod.rs`), so an attacker who keeps the session open can accumulate unbounded data.

## Recommendation

Track per-session upload count and total bytes in the `Manager`; reject new uploads beyond a configurable ceiling (e.g., 50 uploads or 200 MiB per session). Optionally add a per-device rate limit on the upload route.

## Verification

`Manager::store` (lines 143-199) has no aggregate tracking — it reads, validates, generates an ID, and writes. `grep` for `upload.*limit|max_uploads|upload_count|upload_quota` across `src/` returns no quota logic. `close_session` (api/mod.rs:1148-1155) is the only cleanup path, calling `manager.remove_session(&id)`.
