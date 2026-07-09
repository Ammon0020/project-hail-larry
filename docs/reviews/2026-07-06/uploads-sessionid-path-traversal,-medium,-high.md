# Unvalidated sessionID enables path traversal and arbitrary directory deletion

- **Difficulty:** medium
- **Urgency:** high
- **File:** `internal/uploads/uploads.go`
- **Lines:** 64-136 (Store 84, Get 114, RemoveSession 130-136)

## Description

The package carefully validates `uploadID` with `isValidID` (32-char lowercase hex, lines 152-162) to prevent traversal, but `sessionID` is validated only against the empty string (line 65) and is then used directly in `filepath.Join(m.root, sessionID)` for all three operations. `Store` (line 84) and `Get` (line 114) will create/read directories outside the uploads root if sessionID contains `..` or `/`. Most seriously, `RemoveSession` (lines 131-132) calls `os.RemoveAll(filepath.Join(m.root, sessionID))` on this unvalidated path — a sessionID of `..` would delete the uploads root's parent (e.g. `~/.local-agent/`), and `../../foo` deletes arbitrary directories the daemon can reach.

The package doc (lines 1-10) promises "per-session isolated directory" and "preventing path traversal", but that guarantee only holds for uploadID, not sessionID. In the current HTTP wiring the sessionID comes from `r.PathValue("id")` (api.go lines 551, 576, 615) and Go's ServeMux cleans URL paths, which partially mitigates the HTTP entry point — but the package's own API is unsafe, the contract is violated, and any future caller (or a non-ServeMux route, or a sessionID persisted from another source) becomes an arbitrary-deletion primitive. This is a defense-in-depth failure and a real bug in the reusable package.

## Recommendation

Add a `isValidSessionID` guard analogous to `isValidID` (or require sessionIDs to match a safe charset such as `[A-Za-z0-9_-]+` with no path separators and no `..`), and call it at the top of `Store`, `Get`, and `RemoveSession`. Reject empty/invalid sessionIDs with an error before any filesystem operation. Add a unit test for `../` and `/` sessionIDs across all three methods.

## Verification

Read `internal/uploads/uploads.go` — grep for `sessionID` shows 8 usages; the only validation is `if sessionID == ""` at line 65. `isValidID` (lines 152-162) is applied only to uploadID at line 109, never to sessionID. `RemoveSession` (line 132) passes the joined path straight to `os.RemoveAll`. The HTTP handlers at api.go:551 (handleCloseSession → RemoveSession at 562), 576 (handleUpload → Store at 591), and 615 (handleServeUpload → Get at 618) all feed `r.PathValue("id")` directly in. Independently re-verified by reading uploads.go lines 60-139.
