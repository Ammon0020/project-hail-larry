# API client split

> Difficulty: small. Urgency: low. Status: pending.

## Goal

Split `web/src/lib/api.ts` into domain clients without changing caller imports
or request behavior.

## Scope

Use a barrel at `web/src/lib/api/index.ts` and preserve the existing `@/lib/api`
entry point:

```text
web/src/lib/api/
├── index.ts       re-exports
├── client.ts      fetch, errors, auth headers
├── workspaces.ts
├── git.ts
├── sessions.ts
├── permissions.ts
├── mcp.ts
├── providers.ts
└── profiles.ts
```

Keep shared fetch/auth behavior in `client.ts`. Do not split `useBackend.ts` in
this task; its WebSocket and startup orchestration should remain centralized.

## Acceptance

- Existing imports compile unchanged through the barrel.
- URLs, headers, payloads, error statuses, and response types are unchanged.
- Frontend lint and build pass.

## Verification

```text
npm run lint --silent
npm run build --silent
make check
```
