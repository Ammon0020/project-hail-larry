# Attachment-to-ContentBlock translation in Transport.Prompt is untested

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `internal/acp/transport.go`
- **Lines:** 528-560

## Description

The diff adds a substantial new code path to `Transport.Prompt` — a loop over `attachments` that performs file I/O (`os.ReadFile`), base64 encoding, capability-gated branching (`t.promptCaps.Image`), and a fallback path emitting `ResourceLinkBlock` + `TextBlock`. None of this is exercised by any test. Every caller in the test suite passes `nil` for `attachments` (`acp_test.go:135,164`, `integration_test.go:93,226,232,299,388`). The `mockTransport.Prompt` (`lifecycle_test.go:67`) explicitly discards the argument. The `mockagent` (`cmd/mockagent/main.go:43-45`) does not advertise `PromptCapabilities.Image`, so even the integration tests would only hit the fallback branch — and they pass no attachments anyway. This means: (a) the inline `ContentBlockImage` construction is never validated against a real agent, (b) the base64 encoding is never verified, (c) the fallback `ResourceLinkBlock` path is never exercised with real attachments, and (d) the `os.ReadFile` error fallback is never triggered. AGENTS.md requires running tests before marking complete; the new logic has zero coverage.

## Recommendation

Add a unit test for `Transport.Prompt` (or a focused helper that builds the block slice) covering: (1) image-capable agent with a valid image file → asserts an `Image` block with correct `Data`/`MimeType`/`Uri`; (2) image-capable agent with a missing file → asserts fallback `ResourceLink` + `Text` blocks; (3) non-image-capable agent → asserts `ResourceLink` + `Text` blocks. Consider extending `mockagent` to advertise `PromptCapabilities.Image: true` and echo received block types so the integration test can verify block construction end-to-end.

## Verification

Grepped the entire `internal/acp` package for `Attachment|promptCaps|ImageBlock|attachments` — the only references to the new logic are in `transport.go` (the implementation) and the `mockTransport.Prompt` signature change. No test constructs a non-nil `[]interfaces.Attachment` or asserts on produced `ContentBlock`s. Confirmed `mockagent` advertises no `PromptCapabilities` (`cmd/mockagent/main.go:43-45`).
