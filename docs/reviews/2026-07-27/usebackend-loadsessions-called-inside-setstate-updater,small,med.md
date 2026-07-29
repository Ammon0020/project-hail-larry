- name: loadSessions() invoked inside a setSessions updater — impure updater, double renders, StrictMode hazard
- file: /media/adam/extex/projects/project-hail-larry/web/src/hooks/useBackend.ts
- lines: 256-265
- description: |
    In the `ws.onmessage` handler for `SessionCreated`, the code calls
    `loadSessions()` from **inside** the `setSessions` updater function:

    ```ts
    setSessions((prev) => {
      if (!prev.some((s) => s.id === id)) {
        void loadSessions()   // <-- setState triggered from within an updater
      }
      return prev
    })
    ```

    React state updater functions must be pure — they may be invoked more
    than once (StrictMode double-invokes them in dev) and should not trigger
    side effects or other setState calls. Calling `loadSessions()` (which
    itself calls `setSessions`) from within the updater can cause
    double-fetches, dev-only warnings, and in StrictMode an infinite-looking
    render loop because the second invocation sees the same `prev` and
    re-triggers the fetch.

    The intent ("refresh only when the id is unknown") can be achieved
    without nesting: read `sessionsRef` (a ref mirroring `sessions`, the same
    pattern already used for `eventsRef` / `activeWorkspaceRef`) to decide
    whether to call `loadSessions()`, and call it outside the updater.

    While here, the same `SessionCreated` branch also pushes `id` into
    `pendingCreatedSessionIds` unconditionally on every broadcast — including
    to the client that just created the session via REST. That client already
    has the session; the queue entry is harmless (ChatPanel drains it) but it
    causes an unnecessary tab-open effect on the creator. Consider skipping
    the queue push when the session is already in `sessions`.
- verification: |
    Read lines 256-265; `void loadSessions()` is lexically inside the
    `setSessions((prev) => { ... })` arrow. `loadSessions` (lines 422-428)
    calls `setSessions(await api.listSessions())`. React's updater contract
    (https://react.dev/reference/react/useState#storing-information-from-previous-renders)
    requires updaters to be pure.
