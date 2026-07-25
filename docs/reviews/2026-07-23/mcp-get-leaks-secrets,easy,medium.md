# GET /api/mcp returns raw mcp.json including literal secrets

- **Difficulty:** easy
- **Urgency:** medium
- **File:** `src/api/mcp.rs`
- **Lines:** 29-36

## Description

`GET /api/mcp` returns the raw on-disk `mcp.json` bytes via `McpFile::load_raw`. If a user stores literal secrets in MCP server `headers` (e.g. `"Authorization": "Bearer sk-ant-..."`) or `env` values, any paired device that calls `GET /api/mcp` receives those secrets in cleartext. There is no redaction. The `ServerConfig` struct (`mcp/mod.rs:69-89`) serializes `headers` and `env` as plain `BTreeMap<String, String>` with no `skip_serializing_if` or redaction logic. By contrast, `GET /api/agents` correctly redacts `command` and `args` (`agent_registry.rs:56-57`).

## Recommendation

Redact `headers` and `env` values in `GET /api/mcp` responses, returning masked placeholders (e.g. `"***"`) or only key names. If the UI needs to display whether a secret is set, return a boolean `hasSecret` per entry rather than the value.

## Verification

`src/api/mcp.rs:31` `McpFile::load_raw(path)` returns raw bytes. `src/mcp/mod.rs:143-151` `load_raw` returns `fs::read(path)` directly. `src/mcp/mod.rs:84-85` `headers` field has no redaction. Compare `src/acp/agent_registry.rs:56-57` which sets `command: String::new()` and `args: Vec::new()` in `list()`.
