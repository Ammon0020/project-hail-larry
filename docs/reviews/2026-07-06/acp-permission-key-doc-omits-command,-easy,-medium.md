# Permission policy key description omits command discriminator

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `docs/reviews/2026-07-06/acp-audit.md` (line 26) and `docs/reference/acp/responsibilities.md` (line 30)
- **Lines:** audit.md:26; responsibilities.md:30

## Description

Both docs state that `allow_always`/`allow_session` decisions are cached "per `(sessionID, tool, target)`" / "keyed by `(sessionID, tool, target)`". The actual cache key in `internal/permissions/permissions.go:33-38` is a **4-tuple** `policyKey{sessionID, toolKind, target, command}`, where `command` is populated for shell/execute tools (those with `target == ""`) via `policyKeyFor` (permissions.go:48-54). The `command` discriminator is the specific mechanism that prevents a shell-command permission bypass — without it, a single `allow_always` for `go test` would auto-approve every subsequent shell command in the session (the code comments at permissions.go:25-32 and 40-47 call this out explicitly). By describing a 3-tuple target-only key, the docs make the system sound like the vulnerable version rather than the fixed one. A future maintainer reading the doc could "simplify" the key back to the bypass-vulnerable form.

## Recommendation

Update both docs to describe the key as `(sessionID, tool, target)` for file tools and `(sessionID, tool, command)` for shell/execute tools (i.e., target is the discriminator for file tools, command for shell tools). Mention that this split is what closes the shell-command bypass.

## Verification

Read `internal/permissions/permissions.go:25-54` — `policyKey` has four fields including `command`, and `policyKeyFor` sets `key.command = req.Command` when `req.Target == ""`. Cross-referenced the doc claims at audit.md:26 and responsibilities.md:30.
