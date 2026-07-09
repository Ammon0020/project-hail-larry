# Duplicate extensionless files (uploads, uploads_test) are copies of the test, not the implementation

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `internal/uploads/uploads`
- **Lines:** 1-107

## Description

The directory contains four files but only two are real: `uploads.go` (6623 B, the implementation) and `uploads_test.go` (3134 B, the tests). The other two — `uploads` (3134 B) and `uploads_test` (3134 B) — are extensionless byte-for-byte duplicates of `uploads_test.go`, not of `uploads.go`. This is misleading: a file literally named `uploads` (the package name) actually contains test code, so anyone grepping the tree would assume it is the implementation. Go ignores non-`.go` files so there is no compile impact, but this is untracked clutter about to be committed and will confuse readers and tooling (e.g. `cat internal/uploads/uploads` shows tests).

## Recommendation

Delete `internal/uploads/uploads` and `internal/uploads/uploads_test`. Keep only `uploads.go` and `uploads_test.go`. Verify with `md5sum`/`diff` that they are duplicates before deleting.

## Verification

`ls -la` shows uploads, uploads_test, and uploads_test.go are all exactly 3134 bytes while uploads.go is 6623 bytes. Reading all four files confirms the three 3134-byte files have identical content (the test source, starting with `package uploads` + `bytes/os/path/filepath/testing` imports and `TestStoreAndDetect`), while uploads.go is the only file containing the `Manager`/`Store`/`detectImage` implementation.
