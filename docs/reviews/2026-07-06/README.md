# Pre-commit Review — 2026-07-06

Review of all uncommitted changes on branch `acp-focus` before commit.

## Scope

16 subagent reviews across 4 batches (4 subagents each), covering:
- Backend: fswatch, uploads, acp, events, server/api, daemon+interfaces, pairing+config+workspace
- Frontend: App+theme, shadcn/ui, chat components, other components, hooks+api+types
- Docs/config: docs, .devin/agents, docs/acp, go.mod+build config

Each finding is in its own file, titled `name, difficulty, urgency`.

## Summary

- **Total findings:** 61
- **High urgency:** 6
- **Medium urgency:** 18
- **Low urgency:** 37

## High Urgency (fix before commit)

| File | Finding | Difficulty |
|------|---------|-----------|
| `uploads-sessionid-path-traversal,-medium,-high.md` | Unvalidated sessionID enables path traversal + arbitrary dir deletion | medium |
| `server-upload-no-body-size-limit,-medium,-high.md` | Upload endpoint has no request body size cap (DoS) | medium |
| `app-file-refresh-omits-workspaceid,-easy,-high.md` | Silent file-refresh omits workspaceId | easy |
| `pairing-migration-expires-old-devices,-medium,-high.md` | Migration backfill from PairedAt expires old devices on upgrade | medium |
| `chat-send-not-gated-on-uploading,-easy,-high.md` | Send button not gated on uploading (race + duplicate sessions) | easy |
| `hooks-uploadfile-not-in-usebackend,-easy,-high.md` | uploadFile not surfaced through useBackend hook | easy |

## Medium Urgency

| File | Finding | Difficulty |
|------|---------|-----------|
| `fswatch-appwrite-suppression-race,-easy,-medium.md` | App-write suppression race | easy |
| `fswatch-case-sensitive-prefix,-medium,-medium.md` | Case-sensitive prefix breaks Windows/macOS | medium |
| `fswatch-synchronous-emit,-medium,-medium.md` | Synchronous emit blocks event loop | medium |
| `events-attachment-path-lost,-medium,-medium.md` | Attachment.Path silently dropped on persistence | medium |
| `server-multipart-temp-files-leaked,-easy,-medium.md` | Multipart temp files leaked | easy |
| `server-paths-leaked-in-errors,-easy,-medium.md` | Absolute paths leaked in error responses | easy |
| `daemon-uploads-not-cleaned-on-shutdown,-medium,-medium.md` | Uploads not cleaned on daemon shutdown | medium |
| `acp-attachment-translation-untested,-easy,-medium.md` | Attachment translation untested | easy |
| `workspace-onwrite-after-write-toctou,-medium,-medium.md` | onWrite suppression registered after write (TOCTOU) | medium |
| `pairing-sliding-window-test-invalid,-easy,-medium.md` | Sliding-window test doesn't test renewal | easy |
| `theme-system-no-os-change-listener,-medium,-medium.md` | System theme doesn't subscribe to OS changes at runtime | medium |
| `indexcss-select-chevron-hardcoded-hex,-easy,-medium.md` | Select chevron hardcoded hex doesn't adapt to theme | easy |
| `app-banners-not-announced-to-sr,-easy,-medium.md` | Reconnecting/save banners not announced to screen readers | easy |
| `app-resize-handles-not-keyboard-accessible,-medium,-medium.md` | Panel resize handles not keyboard accessible | medium |
| `chat-pending-attachments-leak-across-sessions,-easy,-medium.md` | Pending attachments leak across session switches | easy |
| `editorpane-amber-raw-colors,-easy,-medium.md` | Changed-on-disk banner uses raw amber colors | easy |
| `hooks-duplicated-appevent-interface,-medium,-medium.md` | Duplicated AppEvent interface with divergent shapes | medium |
| `docs-known-issues-broken-review-ref,-trivial,-medium.md` | known-issues.md references non-existent review dir | trivial |
| `acp-permission-key-doc-omits-command,-easy,-medium.md` | Permission policy key doc omits command discriminator | easy |
| `devin-small-agent-missing-dev-rules,-easy,-medium.md` | small agent missing development rules reference | easy |
| `devin-trivial-agent-ambiguous-instruction,-easy,-medium.md` | trivial agent ambiguous self-referential instruction | easy |

## Low Urgency

See individual files in this directory. Categories: fswatch edge cases (3), uploads cleanup (2), events tests (1), acp logging/race (2), server defense-in-depth (1), interfaces design (3), daemon comment mismatch (1), config defaults (1), frontend React/a11y/conventions (12), docs cross-refs (2), acp audit accuracy (3), devin/agents overlap (2).
