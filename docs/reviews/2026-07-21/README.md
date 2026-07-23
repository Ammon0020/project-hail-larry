# Review — 2026-07-21 — Profiles-over-ACP Epic

## Scope

Reviewed the profiles-over-ACP epic diff (`1a849bc..HEAD`, 24 files, ~1424
insertions) across 3 parallel review agents:

| Agent | Profile | Area |
|-------|---------|------|
| Rust ACP + API core | `primary-a` | `src/acp/core.rs`, `src/acp/providers.rs`, `src/api/mod.rs`, `src/interfaces/traits.rs` |
| Frontend | `small` | `web/src/lib/api.ts`, `ChatComposer`, `ChatPanel`, `ProfilesSettings`, `SettingsPanel`, `App`, `EditorPane`, `useBackend` |
| Tests + docs + contract | `small` | `tests/acp_core_lifecycle.rs`, `tests/contract_runner/`, golden JSON, `docs/STATUS.md`, plan files, `AGENTS.md` |

All 4 high-urgency findings were spot-verified against the actual code by the
coordinating agent.

## Summary

**15 findings total** — 4 high, 5 medium, 6 low. 0 critical.

The epic is functionally shipped and all verification (cargo test, clippy, fmt,
contract, npm build) is green, but **4 high-urgency issues should be addressed
before this work is considered production-ready**. Two are real functional bugs
(new-chat profile drop, tools-input comma stripping), one is an architectural
gap (live profile switch doesn't rebind the tool whitelist), and one is a docs
hygiene miss (S-PROF-TOOLS story file not renamed).

## High urgency (4)

| Finding | Difficulty | File |
|---------|-----------|------|
| [Live profile switches do not apply the tool whitelist](profile-switch-does-not-apply-tool-whitelist,hard,high.md) | hard | `src/acp/core.rs` |
| [New-chat profile selection is silently dropped on first send](newchat-profile-selection-silently-dropped,medium,high.md) | medium | `web/src/components/ChatPanel.tsx` |
| [Tools whitelist input strips commas on every keystroke](tools-whitelist-input-strips-commas,medium,high.md) | medium | `web/src/components/ProfilesSettings.tsx` |
| [S-PROF-TOOLS story file not renamed to done-](s-prof-tools-not-renamed-to-done,trivial,high.md) | trivial | `docs/plans/profiles-over-acp/` |

## Medium urgency (5)

| Finding | Difficulty | File |
|---------|-----------|------|
| [Rebind profile capability cache on set_config_option](rebind-profile-capability-cache,easy,medium.md) | easy | `src/acp/core.rs` |
| [Profile switch state is non-atomic](profile-switch-state-is-non-atomic,medium,medium.md) | medium | `src/acp/core.rs` |
| [ChatPanel profile config stale after Settings edit](chatpanel-profile-config-stale-after-settings-edit,easy,medium.md) | easy | `web/src/components/ChatPanel.tsx` |
| [Broken epic cross-references in story files](broken-epic-cross-references,easy,medium.md) | easy | `docs/plans/profiles-over-acp/done-*.md` |
| [Missing fallback test for no-mode-capability agents](missing-fallback-test-no-mode-cap,medium,medium.md) | medium | `tests/acp_core_lifecycle.rs` |

## Low urgency (6)

| Finding | Difficulty | File |
|---------|-----------|------|
| [await_idle helper is dead code](await-idle-dead-code,easy,low.md) | easy | `tests/acp_core_lifecycle.rs` |
| [Duplicate session-creation boilerplate in 4 new tests](duplicate-session-creation-boilerplate,trivial,low.md) | trivial | `tests/acp_core_lifecycle.rs` |
| [ChatComposer profile selector missing aria-label](chatcomposer-profile-selector-missing-aria-label,trivial,low.md) | trivial | `web/src/components/ChatComposer.tsx` |
| [ChatComposer hardcoded Code icon](chatcomposer-hardcoded-code-icon,easy,low.md) | easy | `web/src/components/ChatComposer.tsx` |
| [Profiles list buttons missing aria-current](profiles-list-buttons-missing-aria-current,trivial,low.md) | trivial | `web/src/components/ProfilesSettings.tsx` |
| [Profiles saved-flash timer not cleared on unmount](profiles-savedflash-timer-not-cleared,trivial,low.md) | trivial | `web/src/components/ProfilesSettings.tsx` |

## Notes

- **No type duplication found**: `ProfileConfig`/`ProfileEntry` are defined once
  in `api.ts` and imported by both consumers.
- **No dead exports found**: every new `api.ts` export has a real caller.
- **Contract goldens verified accurate** against the actual handler
  implementations.
- **Test correctness verified**: the 4 new ACP tests check what they claim
  (verified via the mock's `[profile: <id>]` marker and error type assertions).
- The `profile-switch-does-not-apply-tool-whitelist` finding is the largest in
  scope — it's an architectural gap, not a simple bug. A proper fix requires
  either rebinding MCP servers on profile switch or enforcing the whitelist at
  tool invocation time. This may be deferred as a known limitation if the
  current per-server filtering is deemed sufficient for v1.
