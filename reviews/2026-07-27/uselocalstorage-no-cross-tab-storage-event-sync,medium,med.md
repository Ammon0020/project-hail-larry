- name: useLocalStorage does not listen for the `storage` event — multi-tab clients silently diverge
- file: /media/adam/extex/projects/project-hail-larry/web/src/hooks/useLocalStorage.ts
- lines: 18-48
- description: |
    The hook reads once on mount and writes on every update, but never
    subscribes to the window `storage` event. The product is explicitly
    multi-client (AGENTS.md: "Any paired device can answer a pending
    file-write or shell-command permission prompt"; mobile + desktop clients
    on the same LAN). When two browser tabs on the same device (or two
    browsers sharing `localStorage`) are open:

      - Tab A closes chat tab "sess-1" → writes `lai:openTabIds` without
        "sess-1". Tab B's `openTabIds` state is unchanged, so its tab bar
        still shows "sess-1" and `handleCloseTab` operates on stale ids.
      - Tab A toggles word-wrap → `lai:wrap`. Tab B keeps the old wrap state
        until reload.
      - Tab A switches theme → `lai:theme`. `useTheme` has its own
        `matchMedia` listener but no `storage` listener, so Tab B only
        updates on OS preference change, not on sibling-tab theme changes.

    The divergence is silent — there is no error, the user just sees
    inconsistent state across tabs and assumes the app is buggy.

    Fix: add a `storage` event listener in a mount effect that calls
    `setValue` when the event's `key` matches `key` and `newValue` differs
    from the current value (parse + JSON-parse, fall back on error). Guard
    against loops by comparing parsed equality before setting. This makes
    every `useLocalStorage` instance reactive to sibling-tab writes for
    free, which is the behavior users expect from "live" state.

    Minor: `v instanceof Function` (line 35) is a cross-realm-fragile check;
    `typeof v === 'function'` is the conventional test and handles
    cross-realm frames.
- verification: |
    Read the full file (48 lines): no `addEventListener('storage', ...)`
    anywhere. The setter (32-45) only writes. `useTheme` (useTheme.ts:20-26)
    subscribes to `matchMedia` only. The product's multi-device/multi-tab
    posture is stated in AGENTS.md "State and trust".
