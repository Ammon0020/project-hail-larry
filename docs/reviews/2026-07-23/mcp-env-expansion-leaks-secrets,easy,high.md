# MCP ${VAR} expansion leaks daemon process env secrets to agent

- **Difficulty:** easy
- **Urgency:** high
- **File:** `src/mcp/mod.rs`
- **Lines:** 236-260, 269-301

## Description

`ServerConfig::to_acp` calls `expand_env` on the command, every arg, every env value, and every header value. `expand_env` (line 271) resolves `${NAME}` from `std::env::var(name)` — the daemon's own process environment. A malicious or careless `mcp.json` entry can reference `${DEVIN_API_KEY}`, `${HOME}`, or any other daemon env var, and the expanded secret value is serialized into the ACP `session/new` `mcpServers` JSON-RPC message sent to the agent. This puts raw secret values into ACP protocol messages (which may be logged by the agent) and into the agent's process memory as parsed JSON. Combined with the agent env leak (agent-env-hijack-vars), this is a second independent leak path.

## Recommendation

Do not expand `${VAR}` from the daemon's process env for MCP server configs. If env expansion is needed, restrict it to an explicit allowlist of non-secret vars (e.g. `HOME`, `PATH`), or require the user to set the literal value in `mcp.json`. At minimum, refuse to expand vars matching common secret patterns (`*_KEY`, `*_TOKEN`, `*_SECRET`, `*_PASSWORD`, `DEVIN_*`).

## Verification

`src/mcp/mod.rs:239` `expand_env(&self.command)`, line 241 `map_to_env(&self.env)` → line 258 `expand_env(value)`. Line 272: `std::env::var(name).ok()`. Line 2563 in `core.rs` calls `file.to_acp(caps)` and the result is attached to `session/new`. `src/acp/autodetect/devin.rs:174` confirms `DEVIN_API_KEY` is read from `std::env::var`, proving the daemon's env contains secrets.
