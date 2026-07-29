- name: useChatTabs calls setState during render to derive tab list — fragile, extra renders, discouraged by React docs
- file: /media/adam/extex/projects/project-hail-larry/web/src/hooks/useChatTabs.ts
- lines: 22-34 (active-session sync), 36-64 (sessions reconciliation)
- description: |
    The hook uses the "store previous prop in state and setState during
    render when it changes" pattern for both `activeSessionId` and
    `sessions`:

    ```ts
    const [prevActiveSessionId, setPrevActiveSessionId] = useState(activeSessionId)
    if (activeSessionId !== prevActiveSessionId) {
      setPrevActiveSessionId(activeSessionId)
      if (activeSessionId && sessions.some(...)) {
        setOpenTabIds(...)   // setState during render
      }
    }
    ```

    This is the exact pattern React's docs call out as "adjusting state when
    a prop changes" — it works, but it (a) forces an immediate re-render of
    the component before the browser paints, (b) is easy to break by adding
    another dependent branch, and (c) composes poorly with StrictMode
    double-invocation. The `sessions !== prevSessions` reference check
    (line 39) additionally assumes the parent keeps `sessions` referentially
    stable across renders where nothing changed; `useBackend` happens to
    satisfy that today, but any future refactor that recreates the array
    would cause this branch to fire every render and thrash `localStorage`.

    Recommended refactor: move both reconciliations into a single
    `useEffect` keyed on `[activeSessionId, sessions]`. The effect runs after
    render, so it does not cause the synchronous second render, and the
    `openTabIds` state still updates before the user sees anything (effects
    fire before paint when they mutate state that was just set). If the
    "tab must appear in the same commit" requirement is strict, use the
    `useMemo`/derived-state approach instead: compute `openTabIds` as a
    derivation of `sessions` + a persisted user-closed set, rather than
    imperatively mutating a persisted array.
- verification: |
    Read lines 22-64: both `setPrevActiveSessionId` and `setOpenTabIds` are
    called directly in the render body (not inside an effect or callback).
    `setOpenTabIds` comes from `useLocalStorage` (line 20), whose setter
    calls `setValue` (useLocalStorage.ts:34) — a React state setter. React
    docs: "You might need to adjust state when a prop changes ... call the
    set function during rendering" is the documented-but-discouraged escape
    hatch; the effect-based approach is preferred.
