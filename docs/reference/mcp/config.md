# MCP Server Configuration Format

Reference for `~/.local-agent/mcp.json` — the global MCP server config file. See [transports.md](transports.md) for choosing a transport, [env-vars.md](env-vars.md) for `${VAR}` expansion, and [compatibility.md](compatibility.md) for how this maps to other editors.

## File location

```
~/.local-agent/mcp.json
```

This file is **separate** from `~/.local-agent/config.json` (app settings). Keeping MCP config in its own file means the whole file is the portable artifact — `cp friend/mcp.json ~/.local-agent/mcp.json` works. The file is created with mode `0600`; the directory is `0700`.

MCP servers are configured globally and passed to agents on `session/new`, `session/load`, and `session/resume`. There is no live add/remove in ACP v1 — changing config requires starting a new session or resuming. See [§ Limitations](#limitations).

## Envelope structure

```json
{
  "$schema": "https://local-agent.dev/schemas/mcp.json",
  "version": 1,
  "mcpServers": {
    "<name>": { "...per-server fields..." }
  }
}
```

| Field | Type | Description |
|---|---|---|
| `$schema` | string (optional) | URL to the JSON Schema. Powers autocomplete in VS Code / our CodeMirror editor. |
| `version` | int | Config format version. Currently `1`. Lets us migrate later. |
| `mcpServers` | map of `name → ServerConfig` | The server map. **This inner map is Claude Desktop / Cursor / Windsurf compatible** — copy/paste of a per-server object works between editors. See [compatibility.md](compatibility.md). |

## Per-server fields

Each value under `mcpServers` is a `ServerConfig`:

| Field | Type | Default | Applies to | Description |
|---|---|---|---|---|
| `type` | `"stdio"` \| `"http"` \| `"sse"` | `"stdio"` | all | Transport. Omitted ⇒ stdio (matches Claude Desktop). |
| `command` | string | — | stdio | Executable to spawn, e.g. `"npx"`, `"node"`, `"python"`. |
| `args` | string[] | `[]` | stdio | Arguments passed to `command`. |
| `env` | map of `K → V` | — | stdio | Environment variables for the subprocess. Values may contain `${VAR}` — see [env-vars.md](env-vars.md). |
| `cwd` | string | — | stdio | Working directory for the subprocess. Optional. |
| `url` | string | — | http, sse | Endpoint URL of the remote server. |
| `headers` | map of `K → V` | — | http, sse | HTTP headers. Values may contain `${VAR}`. |
| `enabled` | bool | `true` | all | Our extension. `false` filters the server out before it's sent to ACP — the entry is kept, not deleted. Other editors ignore this field. |

> **Note on `env` / `headers` shape:** On disk these are Claude-style `{"KEY":"val"}` maps. ACP's on-the-wire format uses arrays of `{name, value}`. The daemon translates at session-start time — you always write maps in the config file.

## Examples

### stdio (most common)

Local subprocess. All ACP agents MUST support stdio.

```json
{
  "$schema": "https://local-agent.dev/schemas/mcp.json",
  "version": 1,
  "mcpServers": {
    "filesystem": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/me/projects"],
      "env": {}
    }
  }
}
```

### http

Remote server, streaming via HTTP. Requires the agent to advertise `mcp_capabilities.http`.

```json
{
  "$schema": "https://local-agent.dev/schemas/mcp.json",
  "version": 1,
  "mcpServers": {
    "linear": {
      "type": "http",
      "url": "https://mcp.linear.app/mcp",
      "headers": { "Authorization": "Bearer ${LINEAR_TOKEN}" },
      "enabled": true
    }
  }
}
```

### sse

Remote server, Server-Sent Events. Requires the agent to advertise `mcp_capabilities.sse`.

```json
{
  "$schema": "https://local-agent.dev/schemas/mcp.json",
  "version": 1,
  "mcpServers": {
    "remote-tool": {
      "type": "sse",
      "url": "https://tools.example.com/sse",
      "headers": {}
    }
  }
}
```

More drop-in examples: [examples/](examples/) — [github](examples/github.json), [linear](examples/linear.json), [filesystem](examples/filesystem.json), [postgres](examples/postgres.json), [brave-search](examples/brave-search.json).

## The `enabled` field

`enabled` is **our extension** — other editors (Claude Desktop, Cursor, Windsurf) do not recognize it and simply ignore unknown fields. Semantics:

- Default `true` when omitted. Pasted Claude Desktop config is fully enabled by default.
- `false` keeps the entry in the file but filters it out before sending to ACP. Use this to temporarily disable a server without losing its config.
- Deleting a server is a separate, explicit action (remove the key from `mcpServers`).

Toggling `enabled` on a running session does **not** affect that session — see [Limitations](#limitations).

## Limitations

- **No live updates.** ACP only accepts `mcpServers` on `session/new`, `session/load`, and `session/resume`. Changing config requires a new session, or resuming if the agent advertises `sessionCapabilities.resume`.
- **Capability filtering.** At session start, enabled servers are filtered by the agent's advertised `mcp_capabilities`: `http` servers are dropped if the agent doesn't support http, likewise `sse`. stdio is always supported (spec: "All Agents MUST support this transport"). Unsupported servers are skipped with a warning, not failed.
- **Secrets.** Don't inline API keys. Use `${VAR}` references — see [env-vars.md](env-vars.md). The file is mode `0600`, but referenced secrets are safer than committed ones.
