# Uploads manager trusts sessionID for path construction (defense-in-depth gap)

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/uploads/uploads.go`
- **Lines:** 64-66, 108-114, 130-131

## Description

`Manager.Store`, `Get`, and `RemoveSession` build the session directory with `filepath.Join(m.root, sessionID)` and only check `sessionID == ""` in `Store` (not in `Get`/`RemoveSession`). Unlike `uploadID`, which is rigorously validated by `isValidID` (32-char lowercase hex) precisely to "prevent path traversal via crafted upload IDs" (uploads.go:150-151), `sessionID` has no format check. Today the value always comes from `r.PathValue("id")`, and Go's `http.ServeMux` cleans/normalises URL paths so a literal `..` segment can't survive into the value — but this is an implicit, caller-dependent guarantee, not one enforced at the trust boundary (the manager). Any future caller passing a raw query param or JSON field would reintroduce traversal. It also means `handleUpload` happily creates an uploads directory for any arbitrary `sessionID` with no check that the session exists, leaving orphan directories.

Note: This is the same root issue as the high-urgency sessionID path-traversal finding, reported here from the server-side defense-in-depth perspective. The high-urgency finding covers the arbitrary-deletion risk via `RemoveSession`.

## Recommendation

Add a `isValidSessionID` check (mirror `isValidID`, or match whatever format `acp.Client.CreateSession` produces) at the top of `Store`, `Get`, and `RemoveSession`, returning a sentinel error. Optionally have `handleUpload` verify the session exists before storing.

## Verification

Read uploads.go in full — `isValidID` is defined and applied only to `uploadID`; `sessionID` is used unchecked in `filepath.Join` at lines 84, 114, and 131. Confirmed handlers source it from `r.PathValue("id")` (api.go:576, 615) and that no session-existence check precedes `Store`.
