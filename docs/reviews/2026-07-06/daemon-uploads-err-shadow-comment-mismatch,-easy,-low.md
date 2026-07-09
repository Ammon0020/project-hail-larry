# uploads.New error does not abort, but err variable is reused by later code

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/daemon/daemon.go`
- **Lines:** 246-249

## Description

At line 246-249, `uploadsMgr, err := uploads.New(...)` — on failure, err is logged and uploadsMgr is left nil, and execution continues (intentional, per the comment). However, `err` is the same variable used by earlier calls (e.g. appCfg load at line 170 uses a different `err` via `:=`, but `uploads.New` uses `:=` again, redeclaring err in this scope). This is fine in Go, but the pattern is fragile: any future code added between uploads.New and the server.New call that does `if err != nil` would check the uploads error. More importantly, the comment says 'the server handlers return a 503 when Uploads is nil' — handleUpload/handleServeUpload (api.go:572,611) do return 503, but handleSendPrompt (api.go:503-506) returns 400 'uploads not configured' for the attachment case, not 503. The comment is slightly inaccurate about the failure mode. Not a bug, but a documentation/consistency nit.

## Recommendation

Either align handleSendPrompt's attachment-with-nil-uploads response to 503 for consistency with the other upload endpoints, or update the daemon comment to say 'handlers return 4xx/5xx when Uploads is nil'. Minor, but the inconsistency (400 vs 503) could confuse a frontend integrating against the API.

## Verification

Read daemon.go:243-249 comment 'return a 503 when Uploads is nil'. Read api.go:503-506: handleSendPrompt returns 400 'uploads not configured' when Uploads is nil and attachments present. Read api.go:572-574 and 611-613: handleUpload/handleServeUpload return 503. Confirmed inconsistency.
