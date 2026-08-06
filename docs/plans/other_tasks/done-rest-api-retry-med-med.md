# REST API retry for failed send/upload

> **Status:** done | **Difficulty:** medium | **Urgency:** medium
> **Source:** chat reliability audit

## Problem

Only WebSocket has automatic reconnection. REST API failures (send
prompt, upload file, profile switch) require manual retry. If the
daemon is briefly unreachable during a send, the message is lost and
the user must retype and resend.

## Goal

Add automatic retry with exponential backoff for transient REST API
failures (network errors, 5xx). Non-transient failures (4xx) should
not retry.

## Behavior

1. The API client (`web/src/lib/api.ts`) wraps fetch calls with a
   retry layer.
2. Transient failures (network error, 502, 503, 504) retry up to 3
   times with exponential backoff (1s, 2s, 4s).
3. Non-transient failures (400, 401, 403, 404, 409) do not retry.
4. The UI shows a subtle "Retrying…" indicator during retries.
5. If all retries fail, the original error is surfaced.
6. For send-prompt specifically: if the message was lost, the input
   is restored so the user doesn't need to retype.

## Dependencies

- None (frontend-only)

## Acceptance

- [x] Transient failures retry automatically
- [x] Non-transient failures fail immediately
- [x] "Retrying…" indicator shown during retries
- [x] Input restored on final failure
- [x] Tests for retry logic
- [ ] `make check` passes (frontend build/test/lint pass; Rust gate has pre-existing failures unrelated to this frontend-only story)
