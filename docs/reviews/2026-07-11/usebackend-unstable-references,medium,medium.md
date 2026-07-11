# useBackend Hook Returns Unstable Function References

## Location
- [useBackend.ts:519-559](file:///media/adam/extex/projects/project-hail-larry/web/src/hooks/useBackend.ts#L519-L559) — returned object

## Problem

`useBackend()` returns an object with ~25 functions, but these functions are **recreated on every render** because they're defined as plain `async function` or arrow function declarations inside the hook body:

```typescript
async function loadWorkspaces() { ... }
async function loadAgents() { ... }
async function sendPrompt(...) { ... }
// 20+ more
```

None are wrapped in `useCallback`. This means:
1. Every component that receives `backend.someFunction` as a prop will re-render when `useBackend` re-renders (which is every time any piece of backend state changes).
2. Any `useEffect` that includes a backend function in its dependency array will re-fire on every render (hence the eslint-disable comments throughout App.tsx).

The codebase already acknowledges this problem with comments like:
> *"backend's methods are recreated each render (not memoized), so we key on the sessions array + active id"*

## Impact

- Every state change in `useBackend` (e.g. a new WebSocket event) re-creates all 25 functions, potentially triggering cascading re-renders in child components.
- The workaround (`eslint-disable react-hooks/exhaustive-deps` in 4+ places) hides real dependency bugs.
- Effects that should re-run when backend state changes must manually track specific fields instead of the functions, making the dependency model fragile.

## Suggested Fix

Wrap action functions in `useCallback` or, better, extract a stable `actions` object using `useRef` + `useMemo`:

```typescript
const actions = useMemo(() => ({
  loadWorkspaces: async () => { ... },
  sendPrompt: async (...) => { ... },
  // ...
}), []) // stable — actions read state through refs
```

This requires the functions to read current state from refs (which `eventsRef` and `activeWorkspaceRef` already do).
