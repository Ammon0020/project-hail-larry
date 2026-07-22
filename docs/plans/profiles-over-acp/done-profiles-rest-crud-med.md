# Story S-PROF-REST: REST GET/PUT /api/profiles CRUD

> **Status:** pending | **Difficulty:** med
> **Epic:** [profiles-over-acp](../pending-profiles-over-acp-hard.md).
> **Depends on:** S-PROF-CONFIG | **Blocks:** S-PROF-UI, S-PROF-CHAT.

## Goal

Expose the profile config over authenticated REST so the frontend can read and
edit profiles: `GET /api/profiles` returns the resolved config;
`PUT /api/profiles` validates and persists `~/.local-agent/profiles.json`.

## Background / current behavior

- No profile config endpoint exists; profiles are hardcoded (`src/acp/profile.rs`)
  and selection is sent as a REST body field on `/sessions/:id/prompt`
  (`src/api/mod.rs:1041-1047, 1059-1065`) — that field is removed in S-PROF-ACP.
- MCP settings endpoints in `src/api/mod.rs` are a pattern to mirror for
  auth + body caps.

## Desired behavior

- `GET /api/profiles` (auth-gated) → `{ profiles, defaultProfileId }` from the
  S-PROF-CONFIG loader (built-in defaults when no file).
- `PUT /api/profiles` (auth-gated) → validate the full config with the same
  rules as the loader (unknown fields, size caps, count caps, instruction-length
  cap, unsafe tool names, `defaultProfileId` present in `profiles`), write
  atomically (temp file + rename), then reload in-memory config. Reject invalid
  payloads with a structured 4xx and do NOT write.
- Request body size capped; caller is any paired device (single-user trust
  model) — worst case is rewriting local profile instructions/tool whitelist.

## Acceptance criteria

- [ ] `GET /api/profiles` returns built-in defaults when no file exists, and the
      persisted config after a `PUT`.
- [ ] Valid `PUT` writes `~/.local-agent/profiles.json` atomically and updates
      the in-memory config used by `ProfileMiddleware`.
- [ ] Invalid `PUT` (unknown field, oversized, bad tool name, dangling
      `defaultProfileId`) returns 4xx and leaves the existing file untouched.
- [ ] Both endpoints require authentication (off-loopback test) and enforce a
      body-size cap.
- [ ] `cargo test -q --all-targets`, clippy, fmt clean; `make test-contract`
      green (new routes).

## Out of scope

- ACP send path and `/prompt` body change (S-PROF-ACP).
- Live MCP tool enumeration for the editor (S-PROF-TOOLS provides it separately).
