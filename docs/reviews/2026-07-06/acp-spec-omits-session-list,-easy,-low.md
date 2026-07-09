# Session lifecycle reference omits session/list

- **Difficulty:** easy
- **Urgency:** low
- **File:** `docs/reference/acp/spec.md`
- **Lines:** 41-71 (Section 3)

## Description

Section 3 ("Session Lifecycle") documents `session/new`, `session/load`, `session/prompt`, `session/cancel`, and `session/delete`, but does not mention `session/list`. The audit (Gap 7, audit.md:54) refers to "the agent's `session/list` (if supported)" as a real ACP method the client could call to reconcile sessions, and `responsibilities.md:23` references "session/list (our layer)". Since `spec.md` is positioned as the authoritative ACP reference for the project and even documents the unstable `session/delete`, omitting `session/list` (which the audit treats as a legitimate ACP method) leaves the lifecycle section incomplete.

## Recommendation

Add a bullet for `session/list` in Section 3 noting it returns the agent's known sessions (and is the method the client could use to reconcile its session map), marked unstable/optional consistent with `session/delete`.

## Verification

Read spec.md Section 3 (lines 41-71) — only new/load/prompt/cancel/delete are listed. Cross-referenced audit.md:54 (Gap 7 references "session/list" as an ACP call) and responsibilities.md:23 ("session/list (our layer)").
