# Story S-PROF-CONFIG: Profile Config Schema + Loader

> **Status:** done | **Difficulty:** med
> **Epic:** [profiles-over-acp](../pending-profiles-over-acp-hard.md).
> **Depends on:** — | **Blocks:** S-PROF-TOOLS, S-PROF-REST, S-PROF-ACP.

## Goal

Introduce a user-editable `~/.local-agent/profiles.json` and a Rust loader so
profiles (label, instructions, per-tool whitelist) come from config, with the
built-in Code/Ask/Plan defaults as fallback. No behavior change for existing
users (missing file → defaults).

## Background / current behavior

- Profiles are hardcoded: `src/acp/profile.rs:55-75` (`normalize_profile` +
  `instructions_for`). `ProfileMiddleware` (`src/acp/profile.rs`) holds the
  selected profile and injects instructions via `src/acp/context.rs:216-229`.
- Unused `configs/system-messages.json` already carries `profileHeader`,
  `profileCodeInstructions`, `profileAskInstructions`, `profilePlanInstructions`
  — seed the built-in defaults from these strings (or the current hardcoded
  ones; keep text identical to today's output).
- Config-file patterns to mirror: `src/mcp/mod.rs` (load path under
  `~/.local-agent`, deserialize, defaults).

## Desired behavior

- New module `src/acp/profile_config.rs`: typed schema
  `{ profiles: Map<String, Profile>, defaultProfileId: String }` where
  `Profile { label: String, instructions: String, tools: Vec<String> }`.
- Loader reads `~/.local-agent/profiles.json`; on missing file returns the three
  built-in defaults seeded from current strings. On parse error, fail loudly
  (log + surface error) — do NOT silently fall back.
- `ProfileMiddleware` reads resolved config; `normalize_profile` /
  `instructions_for` resolve against config first, built-ins second.
- **Validation on load** (security): reject unknown fields (`deny_unknown_fields`
  or explicit check), cap file size, cap profile count, cap instruction length,
  and reject tool names containing path separators / shell metacharacters /
  whitespace-only. Empty `defaultProfileId` or one not present in `profiles`
  → error.

## Acceptance criteria

- [x] Missing `profiles.json` yields exactly today's Code/Ask/Plan behavior
      (instruction strings byte-identical to `profile.rs:55-75` output).
- [x] A valid custom profile in the file is loadable and its `instructions`
      are injected via `context.rs:216-229` when selected.
- [x] Malformed JSON, unknown fields, oversized file/instructions, too many
      profiles, and unsafe tool names each produce a clear error (unit tests).
- [x] `defaultProfileId` missing from `profiles` map is rejected.
- [x] `normalize_profile` maps unknown/absent selection to `defaultProfileId`.
- [x] `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D
      warnings`, `cargo fmt --check -q` clean.

## Out of scope

- Writing the file (S-PROF-REST) and tool-list validation against live MCP
  tools (S-PROF-TOOLS — schema only validates shape here).
- Any ACP send or REST/UI wiring.
