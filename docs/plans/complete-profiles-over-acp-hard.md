# Epic: Profiles over ACP

> **Status:** complete (server-policy follow-up active). **Owner:** —.
> **Created:** 2026-07-18.
> **Difficulty:** hard. **Depends on:** completed rust-port ACP client
> (`active-rust-port-hard.md`, ACP stories complete). Independent of
> `active-acp-agent-session-history-med.md`.
> **Related:** `src/acp/profile.rs`, `src/acp/context.rs`,
> `src/acp/providers.rs`, `src/mcp/mod.rs`, `src/api/mod.rs`,
> `web/src/components/{ChatComposer,ChatPanel,SettingsPanel}.tsx`,
> `configs/system-messages.json`, `cmd/mockagent/main.go`.
> Stories: `docs/plans/profiles-over-acp/`.

## Goal

Turn "profiles" (Code / Ask / Plan and beyond) into first-class, user-editable
configuration that drives (a) the system instructions injected per turn and
(b) a per-profile MCP-server allowlist, and deliver the selected profile to the agent
over ACP using the spec-preferred `session/set_config_option` (`mode` category)
mechanism instead of the current non-standard REST body field.

## Why an epic

Cross-cutting: new config file + loader, ACP session setup, REST surface change
(with a client migration), Settings UI, chat UX, and the mock agent. Locked
user decisions below make it executable but it spans independently shippable
stories.

## Architecture decisions (LOCKED — do not re-litigate)

- **Custom profiles allowed** beyond Code/Ask/Plan. Schema supports arbitrary
  profile keys, each with a `label`, `instructions`, and an MCP-server policy.
- **MCP-server allowlist** — `mcpServers` names complete configured MCP
  servers. Omitted means all enabled servers; `[]` means none. ACP starts with
  its server list, so changes that alter access require the active transition
  follow-up rather than a live tool rebinding.
- **Per-session persistence** of the selected profile (current behavior). No
  per-workspace or global default beyond `defaultProfileId`.
- **New config file** `~/.local-agent/profiles.json`:
  `{ profiles: { <id>: { label, instructions, mcpServers?: [...] } },
  defaultProfileId: "code" }`. Three built-in defaults (code/ask/plan) seeded
  from today's hardcoded strings (`src/acp/profile.rs:55-75`); missing file →
  built-in defaults (no behavior change for existing users).
- **ACP send path:** when the agent advertises `session/set_config_option` +
  `mode` category, send `SetSessionConfigOptionRequest { category: Mode,
  option: "profile", value: <profile_id> }` on session setup / profile change;
  else fall back to prompt-injection (`src/acp/context.rs:216-229`).
- **REST migration:** remove the non-standard `profile` body field from
  `/sessions/:id/prompt` (`src/api/mod.rs:1041-1047, 1059-1065`) in the same
  release the ACP path lands. Profile is set via ACP set_config_option (when
  advertised) or a dedicated `POST /sessions/:id/profile` endpoint. Clients
  must move off the REST field in that release.
- **Security:** `profiles.json` is written and loaded by the daemon. Validate
  on load — validate profile and configured MCP-server names, cap
  profile count / instruction length / file size, reject unknown fields loudly
  per AGENTS.md security rules.

## Story Index

| ID | Story | Difficulty | Depends on | Status |
|----|-------|-----------|------------|--------|
| S-PROF-CONFIG | [Profile config schema + loader](profiles-over-acp/done-profile-config-schema-med.md) | med | — | ✅ done |
| S-PROF-MOCK | [Mock agent honors set_config_option mode](profiles-over-acp/done-mockagent-set-config-option-easy.md) | easy | — | ✅ done |
| S-PROF-TOOLS | [MCP tool enumeration + per-profile filtering](profiles-over-acp/done-mcp-tool-enumeration-filtering-hard.md) | hard | S-PROF-CONFIG | superseded by server allowlists |
| S-PROF-REST | [REST GET/PUT /api/profiles CRUD](profiles-over-acp/done-profiles-rest-crud-med.md) | med | S-PROF-CONFIG | ✅ done |
| S-PROF-ACP | [ACP set_config_option send + endpoint + drop REST field](profiles-over-acp/done-acp-set-config-option-send-hard.md) | hard | S-PROF-CONFIG, S-PROF-MOCK | ✅ done |
| S-PROF-UI | [Settings Profiles tab](profiles-over-acp/done-settings-profiles-tab-med.md) | med | S-PROF-REST, S-PROF-TOOLS | ✅ done |
| S-PROF-CHAT | [ChatComposer/ChatPanel dynamic list + per-session persistence](profiles-over-acp/done-chat-profile-selection-easy.md) | easy | S-PROF-REST, S-PROF-ACP | ✅ done |

**Suggested sequence:** CONFIG → (MOCK ∥ REST) → ACP → (UI ∥ CHAT).

## Scope

**In scope:** config file + loader, MCP-server policy, ACP set_config_option
send path with capability gate + prompt-injection
fallback, REST CRUD + profile-switch endpoint, removal of the REST `profile`
body field, Settings Profiles tab, chat selector persistence, mock-agent
support.

**Out of scope:** per-workspace / global profile defaults, ACP `session/set_mode`
(deprecated) support, profile import/export/sharing, per-profile model or
harness selection, migrating existing sessions' stored profile (there is none;
selection is transient today).

## Contract note

S-PROF-ACP, S-PROF-REST, and S-PROF-CHAT touch the HTTP/WS surface (routes and
the `/prompt` body shape) and require `make test-contract`.
