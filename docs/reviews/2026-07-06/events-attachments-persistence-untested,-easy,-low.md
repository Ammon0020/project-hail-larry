# Attachments persistence untested

- **Difficulty:** easy
- **Urgency:** low
- **File:** `internal/events/events_test.go`
- **Lines:** 1-235 (entire file — no Attachments test)

## Description

The diff adds Attachments wiring to `Append` (line 115) and `scanEvents` (line 245), but `events_test.go` has no test that appends an event with Attachments and verifies they survive the query round-trip. Existing tests cover tool events (`TestAppendToolEvent`) and permission events (`TestAppendPermissionEvent`) with field-by-field assertions, but the new Attachments field has no equivalent. AGENTS.md development standards require running tests before marking a task complete; this new functionality is untested. This is also the reason the Attachment.Path-lost finding would go undetected — a round-trip test asserting `Path` equality would immediately expose the `json:"-"` data loss.

## Recommendation

Add a `TestAppendAttachmentsEvent` test that appends an `EventPromptSubmitted` with `Attachments: []interfaces.Attachment{{ID: "a1", Name: "img.png", MimeType: "image/png", URI: "file:///tmp/img.png", Path: "/tmp/img.png"}}`, queries it back, and asserts all fields including `Path`. If the Attachment.Path-lost finding is fixed, this test will pass; if not, it will fail and surface the bug.

## Verification

Read the full `events_test.go` file (235 lines). Confirmed no test references `Attachment` or `Attachments`. Grepped `internal/events/events_test.go` for `Attachment` — zero matches.
