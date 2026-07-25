# session_context ignores session_id — global tracker causes cross-session context leakage

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/api/session_extra.rs`
- **Lines:** 97-121

## Description

The handler binds the path parameter as `AxumPath(_session_id)` (line 99 — the underscore prefix means it is intentionally unused). It then calls `state.acp.open_files_tracker()` (line 103) which returns a single process-global `OpenFilesTracker` (defined at `src/acp/context.rs:65` with `RwLock<Vec<String>>` fields — no session keying). This means: (1) Any paired device can overwrite the editor context (open files, recent edits, selection) for ALL sessions by calling `POST /api/sessions/{any-id}/context`. (2) The open-files and selection from one session/device are injected into prompts for other sessions via `PromptPipeline::prepare` (`src/acp/context.rs:355`: `tracker.open_files()` reads the global state). (3) The `session_id` in the URL is never validated for existence — the test at line 320 confirms this by posting to `/api/sessions/sess-1/context` without creating a session. This is a cross-session information disclosure: device A's open file contents can leak into device B's agent prompts.

## Recommendation

Key the `OpenFilesTracker` by `session_id` (e.g., `HashMap<String, OpenFilesTrackerEntry>`) and validate the session exists before updating. Alternatively, if a single global editor state is intentional for the single-user model, document that constraint and at least validate the `session_id` exists.

## Verification

Line 99: `AxumPath(_session_id): AxumPath<String>` — the underscore confirms the parameter is discarded. `OpenFilesTracker` at `src/acp/context.rs:65` has no session keying. `set_open_files` at line 73 directly overwrites the global `RwLock<Vec<String>>`. The `prepare` function at `src/acp/context.rs:350` reads `tracker.open_files()` without any session scoping.
