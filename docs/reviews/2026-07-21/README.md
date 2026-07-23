# Review — 2026-07-21 — Profiles-over-ACP Epic

## Scope

Reviewed the profiles-over-ACP epic diff (`1a849bc..HEAD`, 24 files, ~1424
insertions) across 3 parallel review agents (1 `primary-a` + 2 `small`).

## Summary

**15 findings total** — 4 high, 5 medium, 6 low. 0 critical.

**14 of 15 resolved** and their finding files deleted after verification. The
remaining finding (`profile-switch-does-not-apply-tool-whitelist`) has an
active follow-up story that references the handoff document below; its finding
file and handoff are retained until the story completes.

## Remaining (1)

| Finding | Difficulty | Urgency | Status |
|---------|-----------|---------|--------|
| [Live profile switches do not apply the tool whitelist](profile-switch-does-not-apply-tool-whitelist,hard,high.md) | hard | high | **Active story**: [active-profile-mcp-transition-hard-high.md](../../plans/other_tasks/active-profile-mcp-transition-hard-high.md) — schema migration done (server-level policy), transition dialog pending |

## Handoff

- [HANDOFF-tool-whitelist-on-profile-switch.md](HANDOFF-tool-whitelist-on-profile-switch.md) — original analysis for the external developer

## Resolved and deleted (14)

All 14 finding files were deleted after the fixes were committed and verified
(cargo test, clippy, fmt, contract, npm build, npm lint all clean):

- `await-idle-dead-code` — dead helper removed
- `broken-epic-cross-references` — 7 story file links fixed
- `chatcomposer-hardcoded-code-icon` — Code→Users icon swap
- `chatcomposer-profile-selector-missing-aria-label` — aria-label added
- `chatpanel-profile-config-stale-after-settings-edit` — refresh signal + stale-id fallback
- `duplicate-session-creation-boilerplate` — create_mock_session helper extracted
- `missing-fallback-test-no-mode-cap` — recorded in known-issues, criterion marked [~]
- `newchat-profile-selection-silently-dropped` — pushed on first send
- `profiles-list-buttons-missing-aria-current` — aria-current added
- `profiles-savedflash-timer-not-cleared` — useRef + cleanup effect
- `profile-switch-state-is-non-atomic` — commit after RPC succeeds
- `rebind-profile-capability-cache` — profile_config_id refreshed on rebind
- `s-prof-tools-not-renamed-to-done` — file renamed to done-
- `tools-whitelist-input-strips-commas` — local text buffer + onBlur
