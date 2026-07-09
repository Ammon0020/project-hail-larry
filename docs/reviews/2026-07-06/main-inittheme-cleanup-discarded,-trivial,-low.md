# initTheme() cleanup function discarded — matchMedia listener never removed

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `web/src/main.tsx`
- **Lines:** 9

## Description

main.tsx calls `initTheme()` and ignores the returned cleanup function. initTheme registers a window.matchMedia 'change' listener (theme.ts line 58) when the stored theme is 'system'. Because the cleanup is never invoked, the listener is never removed. In a long-lived SPA this is a single app-lifetime listener (minor), but under Vite HMR or any future re-execution of this module the listener accumulates. More importantly it is a latent leak that breaks the documented contract of the returned cleanup.

## Recommendation

Either capture and invoke the cleanup on module teardown (not strictly possible at the root entry), or move initTheme into a React effect inside App so its cleanup runs on unmount. At minimum, store the cleanup on a module-level guard so HMR can re-run safely.

## Verification

Read main.tsx line 9 (`initTheme()` — return value not captured). Read theme.ts lines 50-60 confirming initTheme returns a cleanup that calls removeEventListener.
