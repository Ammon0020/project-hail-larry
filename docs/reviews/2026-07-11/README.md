# Code Review — 2026-07-11

Coupling, maintainability, and deduplication audit of the `project-hail-larry` repository.

**Convention:** Each finding is a separate file named `<slug>,<difficulty>,<urgency>.md`. Delete the file once the finding is addressed.

## Summary

| # | Finding | Difficulty | Urgency | Area |
|---|---------|------------|---------|------|
| 1 | [Server Deps uses concrete types](server-deps-concrete-types,medium,high.md) | Medium | **High** | Backend |
| 2 | [ACP Client god struct](acp-client-god-struct,hard,high.md) | Hard | **High** | Backend |
| 3 | [App.tsx god component](app-tsx-god-component,medium,high.md) | Medium | **High** | Frontend |
| 4 | [PermissionManager interface incomplete](permission-interface-incomplete,easy,high.md) | Easy | **High** | Backend |
| 5 | [Duplicate session error detection](duplicate-session-error-detection,easy,medium.md) | Easy | Medium | Frontend |
| 6 | [Duplicate frontend type definitions](duplicate-frontend-types,easy,medium.md) | Easy | Medium | Frontend |
| 7 | [Daemon wiring monolith](daemon-wiring-monolith,medium,medium.md) | Medium | Medium | Backend |
| 8 | [ACP Client temporal coupling](acp-client-temporal-coupling,medium,medium.md) | Medium | Medium | Backend |
| 9 | [ChatPanel props explosion](chatpanel-props-explosion,medium,medium.md) | Medium | Medium | Frontend |
| 10 | [Daemon → ACP concrete coupling](daemon-acp-coupling,medium,medium.md) | Medium | Medium | Backend |
| 11 | [useBackend unstable references](usebackend-unstable-references,medium,medium.md) | Medium | Medium | Frontend |
| 12 | [SystemMessages nil-guard duplication](system-messages-nil-guard-duplication,easy,low.md) | Easy | Low | Backend |
| 13 | [Raw ID heuristic cross-language duplication](raw-id-heuristic-cross-language-duplication,easy,low.md) | Easy | Low | Cross |
| 14 | [MCP API pattern inconsistency](mcp-api-pattern-inconsistency,easy,low.md) | Easy | Low | Frontend |
| 15 | [Markdown prose class duplication](markdown-prose-class-duplication,easy,low.md) | Easy | Low | Frontend |

### By urgency

- **High (4):** Items 1-4 — Interface/coupling issues that impact testability and architecture
- **Medium (7):** Items 5-11 — Maintainability issues that add friction but don't block development
- **Low (4):** Items 12-15 — Code style/duplication issues that are easy wins

### Thematic clusters

**Tight coupling (backend):** #1, #4, #8, #10 — The server, daemon, and ACP client reference concrete types instead of the interfaces the project already defines.

**God objects:** #2, #3 — `acp.Client` (1023 lines, 8 responsibilities) and `App.tsx` (841 lines, 14 effects) need decomposition.

**Frontend duplication:** #5, #6, #14, #15 — Types and utility functions are duplicated across files.
