# Server-side absolute paths returned to clients in upload errors

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `internal/server/api.go`
- **Lines:** 511, 580, 593, 620

## Description

Several handlers forward wrapped `os` errors verbatim to the client: `handleSendPrompt` (511: `fmt.Sprintf("attachment %s not found: %v", att.ID, err)`), `handleUpload` (580: `err.Error()` from `ParseMultipartForm`; 593: `err.Error()` from `Store`), and `handleServeUpload` (620: `err.Error()` from `Get`). The uploads manager wraps `os.MkdirAll`/`os.WriteFile`/`os.ReadDir` errors (uploads.go:86, 92, 117), and those `os` errors embed the absolute path — e.g. `mkdir /home/<user>/.local-agent/uploads/<session>: <reason>` or `open /home/<user>/.local-agent/uploads/<session>/<id>.png: no such file`. The root is `filepath.Join(cfg.DataDir, "uploads")` (daemon.go:246), so this discloses the host user's home directory and on-disk layout to any remote paired device. This also deviates from the project convention used elsewhere of returning generic messages (e.g. "invalid request body") and only logging the detail server-side.

## Recommendation

Log the wrapped error server-side and return a generic message to the client ("attachment not found", "upload failed", "invalid upload"). Map `uploads.Manager` errors to sentinels (`os.IsNotExist` → 404, everything else → 400/500) and never `%w`-forward `os` errors to `writeError`.

## Verification

Traced the error chain: `uploads.Store`/`Get` wrap `os.MkdirAll`/`os.WriteFile`/`os.ReadDir` with `%w` (uploads.go:86, 92, 117), and the handlers pass `err.Error()` straight into `writeError`. Confirmed the root path is absolute via `daemon.go:246` (`uploads.New(filepath.Join(cfg.DataDir, "uploads"))`).
