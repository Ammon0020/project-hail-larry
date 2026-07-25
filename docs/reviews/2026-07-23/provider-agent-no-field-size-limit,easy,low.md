# No per-field size limit on provider headers or agent command/args

- **Difficulty:** easy
- **Urgency:** low
- **File:** `src/api/providers.rs`
- **Lines:** 22-29, 45-78; `src/api/mod.rs:927-943`

## Description

`SetProviderRequest` accepts a `headers: HashMap<String, String>` with no per-entry or total size limit beyond the global 10 MiB body cap. A paired device could store thousands of headers with multi-MB values, which are forwarded to the agent via `providers/set` and held in memory. Similarly, `upsert_agent` accepts arbitrarily long `command`, `args`, and `models` vectors with no per-field length cap. The `profiles.json` validator has explicit `MAX_LABEL_CHARS`, `MAX_INSTRUCTION_CHARS`, `MAX_TOOL_NAME_CHARS` limits (`profile_config.rs:32-38`), but agent and provider configs have none.

## Recommendation

Add per-field length limits: cap header count (e.g. 50), header key/value length (e.g. 4 KiB), agent command length (e.g. 1024 chars), args count (e.g. 64), and model count (e.g. 256). Reject oversized fields with 400.

## Verification

`src/api/providers.rs:27-28` `headers: HashMap<String, String>` with `#[serde(default)]` only. No validation in `set_provider` beyond `api_type`/`base_url` non-empty. `src/api/mod.rs:932` only checks `agent.id`/`agent.command` non-empty. Compare `src/acp/profile_config.rs:371` `MAX_LABEL_CHARS` enforcement.
