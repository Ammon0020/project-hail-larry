# Attachment.URI is redundant and inconsistently populated

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/interfaces/interfaces.go`
- **Lines:** 92-105

## Description

Attachment has both URI (json:"uri,omitempty") and Path (json:"-"). The intent per the doc comment is that URI is sent to the agent via ACP ImageBlock.Uri / ResourceLinkBlock.Uri. But the actual population is split: handleSendPrompt (api.go:519) sets URI = "file://" + absPath, and uploads.Manager.Store (uploads.go:100) also produces a URI. The transport (transport.go:537,542,550) only ever reads att.URI — it never reads att.Path for the URI field. Meanwhile att.Path is used only for os.ReadFile in the inline-image branch (transport.go:534). So the contract is muddy: two producers of URI (api.go and uploads.go), and Path is only used in one branch. More importantly, because Attachment is the same struct persisted in Event.Attachments and broadcast over WebSocket, but Path has json:"-" while URI has json:"uri,omitempty", the frontend receives a file:// URI pointing at the user's local filesystem — which the browser cannot fetch (cross-origin/file:// is blocked). The frontend instead uses the /api/sessions/{id}/uploads/{uploadID} URL it got from the upload response (api.go:601). So the URI field in the broadcast event is dead/misleading data sent to every paired device.

## Recommendation

Clarify the contract: either (a) drop URI from the broadcast Event.Attachments (give Attachment a separate transport-only field, or build the URI in the transport from Path), or (b) document that URI is for agent-side use only and add a json:"-" to it as well, exposing only ID/Name/MimeType to the frontend. The frontend should derive its thumbnail URL from ID via the known /api/sessions/{id}/uploads/{uploadID} route. As-is, the field leaks a host filesystem path scheme to all clients for no benefit.

## Verification

Read Attachment struct (interfaces.go:88-105) with URI json:"uri,omitempty" and Path json:"-". Read api.go:514-520 setting both Path and URI. Read transport.go:532-553 using only att.URI and att.Path (Path only for ReadFile). Read api.go:597-603 returning a separate 'url' field to the frontend. Confirmed URI in Event.Attachments is not consumable by the browser.
