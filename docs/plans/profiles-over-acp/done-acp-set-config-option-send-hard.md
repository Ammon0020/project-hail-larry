# Story S-PROF-ACP: ACP set_config_option Send + Profile Endpoint + Drop REST Field

> **Status:** done | **Difficulty:** hard
> **Epic:** [profiles-over-acp](../complete-profiles-over-acp-hard.md).
> **Depends on:** S-PROF-CONFIG, S-PROF-MOCK.
> **Blocks:** S-PROF-CHAT (endpoint + body change).

## Goal

Deliver the selected profile to the agent over ACP via
`session/set_config_option` (`mode` category) when advertised, with a
prompt-injection fallback otherwise; add a dedicated `POST /sessions/:id/profile`
switch endpoint; and REMOVE the non-standard `profile` field from the
`/sessions/:id/prompt` body in the same release.

## Background / current behavior

- ACP v1 has no native `profile` field; spec-preferred mechanism is
  `session/set_config_option` with `category: "mode"`. Types already imported:
  `src/acp/providers.rs:22-25` (`SetSessionConfigOptionRequest`,
  `SessionConfigOptionCategory`, etc.).
- Deprecated alternative: `session/set_mode` + `current_mode_update` — NOT used.
- Today profile is a REST body field on `/prompt`
  (`src/api/mod.rs:1041-1047, 1059-1065`), stored in `ProfileMiddleware`
  (`src/acp/profile.rs`), injected into context (`src/acp/context.rs:216-229`).

## Desired behavior

- Capability gate: if the agent advertises `session/set_config_option` + a
  `mode` category, send `SetSessionConfigOptionRequest { category: Mode,
  option: "profile", value: <profile_id> }` on session setup AND whenever the
  profile changes. Else keep prompt-injection via `context.rs:216-229`.
- New `POST /sessions/:id/profile` (auth-gated) sets the active profile for the
  session, triggering the send path above. Profile id validated against loaded
  config; unknown id → 4xx.
- **REMOVE** `profile` from the `/sessions/:id/prompt` request body
  (`src/api/mod.rs:1041-1047, 1059-1065`). `ProfileMiddleware` selection now
  comes only from the new endpoint (or default).
- **Migration risk (call out to release):** removing the `/prompt` `profile`
  field is a breaking wire change. Clients (this repo's frontend in S-PROF-CHAT)
  MUST move to `POST /sessions/:id/profile` in the same release; document in the
  changelog / STATUS.

## Acceptance criteria

- [x] Agent advertising `mode` config option receives
      `session/set_config_option { option: "profile", value }` on session setup
      and on profile change (verified against mockagent, S-PROF-MOCK).
- [~] Agent WITHOUT the capability falls back to prompt injection; instructions
      still applied (fallback branch is the `profile_config_id == None` path;
      `MOCKAGENT_NO_MODE_CAP=1` fallback test deferred — env wiring per-test is
      not in the existing harness; the capability-present branch is covered by
      `mockagent_initial_profile_sent_over_acp_when_capability_advertised`, but
      the no-capability fallback branch is NOT tested. See
      `docs/known-issues.md` "Profile-over-ACP fallback branch untested").
- [x] `POST /sessions/:id/profile` sets the session profile; unknown id → 400;
      missing session → 404; requires auth (protected router).
- [x] `/sessions/:id/prompt` no longer reads a `profile` body field; sending one
      is silently ignored by serde and does not change behavior.
- [x] Per-session selection persists for the session's lifetime (current
      behavior preserved via the endpoint + middleware).
- [x] `cargo test`, clippy, fmt clean; `make test-contract` green (ACP + REST
      surface changed).

## Out of scope

- Config schema/loader (S-PROF-CONFIG), tool filtering (S-PROF-TOOLS), UI/chat
  wiring (S-PROF-UI, S-PROF-CHAT).
- Deprecated `session/set_mode` support.
