# Silent fallback on attachment file-read error violates "Fail Loudly" / "Logging" conventions

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `internal/acp/transport.go`
- **Lines:** 534-541

## Description

When `t.promptCaps.Image` is true and `os.ReadFile(att.Path)` fails, the code falls back to a `ResourceLinkBlock` + text hint and `continue`s, discarding the error entirely — no `slog` call, no diagnostic. The doc comment states this is intentional ("File read errors fall back to the resource-link path rather than failing the whole prompt"), and the degraded behavior is reasonable. However, the error is not logged anywhere, making it invisible to operators debugging why an image attachment arrived as a text hint. This deviates from the global rules in AGENTS.md ("Fail Loudly" — "If a command fails, it should fail loudly and provide a clear error message") and the "Logging" rule ("Use logging throughout the code to help with debugging"). The transport already imports `log/slog` (line 9) and uses it elsewhere, so a one-line `slog.Warn` is consistent with conventions.

## Recommendation

Log the read error at `slog.Warn` level before the fallback, e.g. `slog.Warn("attachment read failed; falling back to resource link", "path", att.Path, "err", err)`.

## Verification

Read `transport.go:528-560`; the `err` from `os.ReadFile` is checked but never passed to any logger. Confirmed `log/slog` is already imported (line 9) and the file uses `slog.New(...)` at line 450.
