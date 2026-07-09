# uploadFile error-response body is implicit any

- **Difficulty:** trivial
- **Urgency:** low
- **File:** `web/src/lib/api.ts`
- **Lines:** 248-251

## Description

`const body = await res.json().catch(() => ({ error: res.statusText }))` — `res.json()` returns `Promise<any>`, so `body` is `any` and `body.error` is an untyped property access. The linter rule `@typescript-eslint/no-unsafe-member-access` would flag this. It mirrors the pre-existing pattern in `apiFetch` (line 20), but since this is newly added code it's a chance to do better.

## Recommendation

Type the fallback explicitly: `const body: { error?: string } = await res.json().catch(() => ({ error: res.statusText }))` then `throw new Error(body.error || \`HTTP ${res.status}\`)`.

## Verification

`read` of api.ts:248-251 confirms `body` has no type annotation and `.error` is accessed without a cast.
