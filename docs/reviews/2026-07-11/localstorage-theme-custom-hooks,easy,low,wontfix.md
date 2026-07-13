# Custom LocalStorage and Theme React Hooks

- **Difficulty:** easy
- **Urgency:** low
- **File:** `/media/adam/extex/projects/project-hail-larry/web/src/hooks/useLocalStorage.ts`
- **Lines:** 1-49

## Description

The React application implements custom state hooks under `web/src/hooks/`:
1. `useLocalStorage.ts` handles component state synchronization with `window.localStorage` and handles JSON serialization.
2. `useTheme.ts` manages switching between light and dark modes by mutating document element classes.

While these hooks are relatively small, they replicate boilerplate patterns that have standard, edge-case-hardened library solutions.

## Recommendation

Replace these hand-rolled hooks with popular hooks libraries:
- **`usehooks-ts`** (contains a robust `useLocalStorage` hook that handles hydration, cross-tab events, and type safety).
- **`next-themes`** or simple Tailwind dark-mode hooks libraries to standardize dark mode settings, OS system theme detection, and hydration sync.

## Verification

Code inspection of [web/src/hooks/useLocalStorage.ts](file:///media/adam/extex/projects/project-hail-larry/web/src/hooks/useLocalStorage.ts) and [web/src/hooks/useTheme.ts](file:///media/adam/extex/projects/project-hail-larry/web/src/hooks/useTheme.ts) shows custom hook wrappers implementing standard storage-listening and DOM theme class-switching logic.

## Resolution (2026-07-12) — WONTFIX
Both hooks are small, working, documented, and project-specific (`useTheme` is tightly coupled to `@/lib/theme`); `next-themes` targets Next.js while this project uses Vite, and adding two dependencies for ~77 lines of tested user-facing code risks regressions for no functional gain.
