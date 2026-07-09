# Attachment.Path silently dropped during event persistence

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `internal/events/events.go`
- **Lines:** 115, 192, 245

## Description

The diff adds `Attachments []interfaces.Attachment` to `eventPayload`, which is JSON-marshaled into the SQLite `payload` TEXT column on `Append` (line 115) and JSON-unmarshaled on query (line 245). However, `interfaces.Attachment.Path` is tagged `json:"-"` (interfaces.go line 104), which causes `json.Marshal` to **omit the Path field entirely** from the stored JSON. On read-back via `json.Unmarshal`, `Path` will always be the zero value (empty string). The package doc comment (lines 4-6) states the event log is the append-only immutable source of truth from which "application state is derived" — silently dropping a field on persistence violates that contract. The `json:"-"` tag was intended to suppress Path in **frontend** serialization (per the comment "Not serialized to the frontend"), but it also suppresses it in the **internal storage** serialization, which was not the intent. Currently this is latent: no code path reads `Attachments[].Path` from replayed events (frontend uses `URI`, conversation export ignores Attachments). But any future replay/reconnection/re-derivation code that relies on the event log as source of truth will get empty Paths.

## Recommendation

Use a separate serialization for internal storage that includes Path, OR store Path in the event payload via a distinct field. Options: (a) define a storage-specific `attachmentPayload` struct inside `events.go` that includes Path with a real json tag, and map to/from `interfaces.Attachment` in `Append`/`scanEvents`; (b) change `Attachment.Path` from `json:"-"` to `json:"path,omitempty"` and instead suppress it at the HTTP/WS boundary (e.g., in the server's event serialization for the frontend). Option (a) is cleaner and keeps the interface contract unchanged.

## Verification

Read `internal/interfaces/interfaces.go` lines 92-105 confirming `Path string \`json:"-"\``. Read `internal/events/events.go` lines 99-116 (marshal) and 224-245 (unmarshal) confirming `eventPayload.Attachments` uses `interfaces.Attachment` directly, so the `json:"-"` tag applies. Confirmed no storage-specific override exists. Grepped for `.Attachments` usage across `internal/` — no consumer reads `.Path` from store-queried events today, confirming the bug is latent but real.
