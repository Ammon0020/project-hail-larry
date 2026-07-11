# Duplicated "Session Not Found" Error Detection

## Location
- [useBackend.ts:21-24](file:///media/adam/extex/projects/project-hail-larry/web/src/hooks/useBackend.ts#L21-L24) — `isSessionNotFound()`
- [ChatPanel.tsx:23-29](file:///media/adam/extex/projects/project-hail-larry/web/src/components/ChatPanel.tsx#L23-L29) — `isSessionGone()`

## Problem

Two nearly identical functions detect the same condition (backend returned "session not found") with slightly different implementations:

```typescript
// useBackend.ts
function isSessionNotFound(message: string): boolean {
  const lower = message.toLowerCase()
  return lower.includes('session not found') || lower.includes('not found')
}

// ChatPanel.tsx
function isSessionGone(message: string): boolean {
  const lower = message.toLowerCase()
  return (
    lower.includes('session not found') ||
    lower.includes('no longer available') ||
    lower.includes('not found')
  )
}
```

`isSessionGone` additionally checks for "no longer available" (the friendly message thrown by `useBackend.sendPrompt` itself). These are module-private functions that cannot be shared by other consumers that may need the same check.

## Impact

- If the backend error message format changes, two locations must be updated.
- A new component checking for this condition would create a third copy.

## Suggested Fix

Consolidate into a single exported utility in `@/lib/errors.ts`:

```typescript
export function isSessionNotFound(message: string): boolean {
  const lower = message.toLowerCase()
  return (
    lower.includes('session not found') ||
    lower.includes('no longer available') ||
    lower.includes('not found')
  )
}
```

Import it in both `useBackend.ts` and `ChatPanel.tsx`.
