# looksLikeRawId Heuristic Duplicated in Frontend and Backend

## Location
- [ChatMessageItem.tsx:51-65](file:///media/adam/extex/projects/project-hail-larry/web/src/components/ChatMessageItem.tsx#L51-L65) — TypeScript `looksLikeRawId()`
- [transport.go:307](file:///media/adam/extex/projects/project-hail-larry/internal/acp/transport.go#L307) — Go `looksLikeRawID()` (referenced in comment)

## Problem

Both the Go backend and the TypeScript frontend implement the same heuristic for detecting opaque tool-call IDs (e.g. `toolu_01H…`, `call_abc123`, UUIDs). The backend comment even says *"see ChatMessageItem.tsx#looksLikeRawId"* — explicitly linking the two implementations.

The backend uses this to sanitize the tool title before emitting the event. The frontend uses it as a fallback in case the backend's sanitization missed one. They must agree on what constitutes a "raw ID" for the UX to be consistent.

## Impact

- If a new agent introduces a novel ID format, both implementations must be updated in lock-step.
- The duplication is acknowledged in comments but not enforced by tests.

## Suggested Fix

**Option A (preferred):** Have the backend always sanitize the tool title before emitting events. If the backend handles it authoritatively, the frontend can trust the `tool` field and drop its local heuristic entirely.

**Option B:** If the frontend must keep a fallback, add a shared test case file (e.g. `testdata/raw-id-samples.json`) consumed by both Go and TypeScript tests to ensure they stay in sync.
