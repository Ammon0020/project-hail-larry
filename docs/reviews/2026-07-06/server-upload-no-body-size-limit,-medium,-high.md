# Upload endpoint has no request body size cap (DoS / disk exhaustion)

- **Difficulty:** medium
- **Urgency:** high
- **File:** `internal/server/api.go`
- **Lines:** 578-582

## Description

`handleUpload` calls `r.ParseMultipartForm(uploads.MaxUploadBytes)` believing the argument caps the upload size (the comment says "MaxUploadBytes (10 MB) matches the uploads manager's internal cap"). It does not. `ParseMultipartForm(maxMemory)` only sets the threshold above which form data is spilled to **temp files on disk** — it is not a limit on the total request body. The only body-size enforcement in this codebase is `http.MaxBytesReader`, applied inside `decodeJSONLimit` (server.go:414), which the multipart path never invokes. `uploads.Store` does re-read with a `LimitReader(MaxUploadBytes+1)`, but by then `ParseMultipartForm` has already buffered the entire body to temp files. Any authenticated (or loopback) client can stream an arbitrarily large multipart body and fill the temp directory / disk before `Store` ever runs.

## Recommendation

Wrap the body before parsing: `r.Body = http.MaxBytesReader(w, r.Body, uploads.MaxUploadBytes)` (plus a small allowance for multipart framing/headers) at the top of `handleUpload`. Treat `*http.MaxBytesError` as a 413. Keep `Store`'s `LimitReader` as a second layer of defense.

## Verification

Confirmed via grep that `MaxBytesReader` is only used in `decodeJSONLimit` (server.go:414) and that `handleUpload` never wraps `r.Body`. Re-read the Go `http.Request.ParseMultipartForm` contract: the argument is the in-memory threshold, not a body cap, and the docs explicitly state "If the request body's size has not been limited by way of MaxBytesReader, the request body can be arbitrarily large." Independently re-verified by reading api.go lines 568-604.
