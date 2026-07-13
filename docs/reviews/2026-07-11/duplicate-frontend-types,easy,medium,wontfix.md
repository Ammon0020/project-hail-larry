# Duplicated Type Definitions Between api.ts and types/index.ts

## Location
- [api.ts:112-127](file:///media/adam/extex/projects/project-hail-larry/web/src/lib/api.ts#L112-L127) — `SearchOptions`, `SearchResult`
- [types/index.ts:178-203](file:///media/adam/extex/projects/project-hail-larry/web/src/types/index.ts#L178-L203) — `SearchOptions`, `SearchResult`

Also related:
- `AgentInfo` / `Agent` — defined in both `api.ts` and `types/index.ts` with slightly different shapes (api.ts has `command` and `args`; types has neither)
- `SessionInfo` / `Session` — defined in `api.ts` (flat) and `types/index.ts` (with `time`, `active`, typed `SessionStatus`)

## Problem

The frontend has two sources of truth for shared types:
- `@/lib/api.ts` defines types that mirror Go structs (wire format)
- `@/types/index.ts` defines types that the UI components consume (display format)

Some types are **identically duplicated** (`SearchOptions`, `SearchResult`), and others exist as **parallel definitions** with divergent shapes (`AgentInfo` vs `Agent`, `SessionInfo` vs `Session`). `SearchPanel.tsx` imports `SearchResult` from `@/lib/api`, not `@/types`, creating an inconsistency.

## Impact

- Adding a field (e.g. `workspace` to `SearchResult`) requires updating two files.
- Developers can accidentally import the wrong `SearchResult` and get silently wrong types.
- The shape divergence between `AgentInfo` and `Agent` means `App.tsx` must manually map between them (the `agents: backend.agents` prop threading).

## Suggested Fix

1. Remove `SearchOptions` and `SearchResult` from `api.ts`; import them from `@/types`.
2. Unify `AgentInfo`/`Agent` into a single type with optional `command`/`args` fields. `api.ts` re-exports from `@/types`.
3. Use a single `Session` type everywhere (extend with `time`/`active` only where needed as local view-model props, not duplicate interfaces).

## Resolution (2026-07-12) — WONTFIX
Already resolved: api.ts imports/re-exports canonical SearchOptions/SearchResult/Agent/Session from @/types (no duplicate defs; AgentInfo/SessionInfo gone; Agent has optional command/args; Session has time/active/SessionStatus); SearchPanel's SearchResult import via re-export is the same type.
