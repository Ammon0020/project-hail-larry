# MCP Config API Functions Bypass the api Object Pattern

## Location
- [api.ts:331-356](file:///media/adam/extex/projects/project-hail-larry/web/src/lib/api.ts#L331-L356) — `getMcpConfig()`, `putMcpConfig()`, `patchMcpServer()`
- [ChatPanel.tsx:5](file:///media/adam/extex/projects/project-hail-larry/web/src/components/ChatPanel.tsx#L5) — `import { getMcpConfig, patchMcpServer } from '@/lib/api'`

## Problem

The `api.ts` file follows a clear pattern: all API methods are defined as properties of the `api` object (`api.listWorkspaces()`, `api.sendPrompt()`, etc.). However, the MCP config functions break this pattern by being exported as **standalone functions** outside the `api` object:

```typescript
export const api = {
  // 25+ methods on the api object
  ...
}

// These break the pattern:
export async function getMcpConfig(): Promise<string> { ... }
export async function putMcpConfig(rawJson: string): Promise<void> { ... }
export async function patchMcpServer(name: string, enabled: boolean): Promise<void> { ... }
```

Additionally, `ChatPanel.tsx` imports and calls these directly instead of routing through `useBackend`, which means MCP operations bypass the hook's error handling, session-recovery semantics, and loading-state management.

## Impact

- Inconsistent API: some calls go through `api.method()` → `useBackend` → component; MCP calls go directly from the component to standalone functions.
- MCP errors in ChatPanel are caught with local `try/catch` instead of the hook's centralized error handling.

## Suggested Fix

1. Move `getMcpConfig`, `putMcpConfig`, `patchMcpServer` onto the `api` object.
2. Add corresponding wrappers in `useBackend` (or a dedicated `useMcpConfig` hook).
3. Have ChatPanel call through the hook instead of importing `api` directly.
