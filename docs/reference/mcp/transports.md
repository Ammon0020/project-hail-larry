# MCP Transports: stdio vs http vs sse

When to use each transport. For the field-level config format see [config.md](config.md).

## stdio

Local subprocess. The daemon spawns a process and speaks MCP over its stdin/stdout.

- **When to use:** Local tools — filesystem, git, shell, databases on localhost.
- **Agent support:** All ACP agents **MUST** support stdio (spec: "All Agents MUST support this transport"). This is the only transport guaranteed to work with every agent.
- **Config:** `command`, `args`, `env`, `cwd`. Omit `type` (defaults to `stdio`).

```json
{
  "filesystem": {
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-filesystem", "/home/me/projects"]
  }
}
```

## http

Remote server, streaming via HTTP. The agent connects to a URL and streams responses over HTTP.

- **When to use:** Hosted services — Linear, GitHub (hosted), any remote MCP endpoint.
- **Agent support:** Requires the agent to advertise `mcp_capabilities.http`. If unsupported, the server is skipped at session start.
- **Config:** `type: "http"`, `url`, `headers`.

```json
{
  "linear": {
    "type": "http",
    "url": "https://mcp.linear.app/mcp",
    "headers": { "Authorization": "Bearer ${LINEAR_TOKEN}" }
  }
}
```

## sse

Remote server, Server-Sent Events. The agent connects to a URL and receives server-pushed events over an SSE channel.

- **When to use:** Hosted services that expose SSE rather than streaming HTTP. Functionally similar to http; use whichever the server documents.
- **Agent support:** Requires the agent to advertise `mcp_capabilities.sse`. If unsupported, the server is skipped at session start.
- **Config:** `type: "sse"`, `url`, `headers`.

```json
{
  "remote-tool": {
    "type": "sse",
    "url": "https://tools.example.com/sse",
    "headers": {}
  }
}
```

## Recommendation

| Use case | Transport |
|---|---|
| Local tools (filesystem, git, shell) | **stdio** |
| Local databases (postgres on localhost) | **stdio** |
| Hosted services (Linear, GitHub) | **http** or **sse** (per the server's docs) |
| Remote MCP endpoint of unknown type | Check the server's docs; default to **http** |

Prefer **stdio** whenever the tool runs locally — it's the only universally supported transport and avoids network round-trips. Use **http**/**sse** only for hosted services you can't run as a local subprocess.

See [compatibility.md](compatibility.md) for how these transports map across editors, and [examples/](examples/) for drop-in configs.
