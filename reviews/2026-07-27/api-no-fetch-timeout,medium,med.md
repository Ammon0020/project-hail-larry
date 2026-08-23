- name: No fetch timeout or AbortController — UI hangs indefinitely on a slow/hung daemon
- file: /media/adam/extex/projects/project-hail-larry/web/src/lib/api.ts
- lines: 98-118, 438-451, 468-488, 529-538, 624-638
- description: |
    `apiFetch` (line 99) and every direct `fetch` call (`uploadFile`,
    `exportSession`, `getMcpConfig`, `listProviders`) pass no `signal` and no
    timeout. On a LAN with a flaky WiFi connection, a paused daemon process,
    or a slow agent subprocess holding a backend lock, the request never
    resolves and never rejects — the UI sits in a pending state with no
    spinner, no cancel button, and no error.

    Concrete UX failures:
    - `saveFile` (line 335) hangs → the editor's "saving…" indicator never
      clears and the user cannot tell whether their edit was persisted.
    - `sendPrompt` (line 425) hangs → the chat input stays disabled with no
      feedback that the prompt was never delivered.
    - `uploadFile` (line 438) hangs → the attachment spinner spins forever.

    For a self-hosted IDE where the daemon is on the same LAN (not a CDN with
    30s SLAs), a default timeout of ~20-30s with an AbortController would let
    the UI show "the daemon isn't responding — check it's running" instead of
    looking frozen. Callers that need longer waits (large uploads, exports)
    should be able to opt into a longer timeout or pass their own signal.

    Suggested approach: give `apiFetch` an optional `timeoutMs` (default
    ~20s), create an `AbortController` internally, and race `fetch` against
    `setTimeout(() => controller.abort(), timeoutMs)`. Throw a typed
    `ApiTimeoutError` so the UI can show "request timed out" distinctly from
    a server error.
- verification: |
    Read api.ts in full — no `AbortController`, `signal`, or `setTimeout`
    appears anywhere in the file. All fetch calls await without a deadline.
