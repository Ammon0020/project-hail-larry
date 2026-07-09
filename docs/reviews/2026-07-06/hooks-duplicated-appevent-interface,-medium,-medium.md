# Duplicated AppEvent interface with divergent id optionality and type strictness

- **Difficulty:** medium
- **Urgency:** medium
- **File:** `web/src/lib/api.ts` (lines 72-91) and `web/src/types/index.ts` (lines 125-146)

## Description

Two `AppEvent` interfaces coexist. In `api.ts`: `id: number` (required) and `type: string`. In `types/index.ts`: `id?: number` (optional) and `type: EventType` (union). The diff adds `attachments?: Attachment[]` to **both**, perpetuating the divergence. `useBackend` imports `AppEvent` from `@/lib/api` (useBackend.ts:2) while `useMockBackend` imports it from `@/types` (useMockBackend.ts:2), so the real and mock backends are typed against structurally different event shapes. This is a type-safety gap: a mock event with `id` omitted and `type: 'FileChangedOnDisk'` is assignable to the mock's `AppEvent` but not to the API client's, and a real event with `type: string` bypasses `EventType` exhaustiveness checks in switch statements that consumers write against `@/types`.

## Recommendation

Delete the `AppEvent` (and other duplicated) interface from `api.ts` and re-export from `@/types`, or vice versa. Make the API client import `AppEvent` from `@/types` so there is a single source of truth. If the backend truly always sends `id`, model that as `id?: number` once (matching the mock's looser shape) rather than maintaining two definitions.

## Verification

`read` of both files confirms the two definitions differ on `id` optionality and `type` strictness; `grep` confirms the two hooks import from different modules.
