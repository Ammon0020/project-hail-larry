# Multipart temp files leaked on every upload

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `internal/server/api.go`
- **Lines:** 579-595

## Description

`ParseMultipartForm` spills file parts to temp files under `os.TempDir()`. `r.FormFile` returns a handle to that temp file, which is `defer`-closed, but the temp file itself is only deleted by `r.MultipartForm.RemoveAll()`, which is never called. `Store` copies the bytes into the uploads directory, so the temp copy is pure waste. Combined with the no-body-size-limit finding, every upload (and every aborted/oversized upload attempt) leaves a file behind in the system temp dir for the lifetime of the process.

## Recommendation

After `Store` succeeds (or in a `defer`), call `if r.MultipartForm != nil { _ = r.MultipartForm.RemoveAll() }`. Better, stream the file part directly via `r.MultipartReader()` instead of `ParseMultipartForm` so temp files are never materialised.

## Verification

Read the full handler (api.go:568-604) — there is no `RemoveAll` call. The Go `multipart.Form` contract states temp files persist until `RemoveAll` is invoked; `net/http` does not auto-clean them.
