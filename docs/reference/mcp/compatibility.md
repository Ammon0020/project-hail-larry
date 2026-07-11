# MCP Config Compatibility with Other Editors

How `~/.local-agent/mcp.json` relates to other editors' MCP config formats. For our format see [config.md](config.md).

## The short version

The `mcpServers` inner map in our file is **byte-compatible** with Claude Desktop, Cursor, Windsurf, and Claude Code. To copy a server between editors, copy the inner object under `mcpServers` — give it a key in the destination.

## Editor reference

| Editor | File | Top-level key | Compatible? |
|---|---|---|---|
| **Claude Desktop** | `claude_desktop_config.json` (platform-specific path) | `mcpServers` | ✅ Inner map is byte-compatible |
| **Cursor** | `~/.cursor/mcp.json` | `mcpServers` | ✅ Same format |
| **Windsurf** | `~/.codeium/windsurf/mcp_config.json` | `mcpServers` | ✅ Same format |
| **Claude Code** | `~/.claude.json` + `.mcp.json` (project) | `mcpServers` | ✅ Same format, supports `${VAR}` |
| **VS Code** | `.vscode/mcp.json` | `servers` (not `mcpServers`) | ⚠️ Outlier — see below |
| **Us** | `~/.local-agent/mcp.json` | `mcpServers` (inside an envelope) | ✅ Superset — see [config.md](config.md) |

## How to copy a server block between editors

The per-server object is the portable unit. To move a server from one editor to another:

1. In the source editor, copy the object under a server name in `mcpServers`.
2. In the destination, paste it as the value of a new key under `mcpServers` (or `servers` for VS Code).

**From Claude Desktop to us:**

```jsonc
// Claude Desktop: claude_desktop_config.json
{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "ghp_xxx" }
    }
  }
}
```

Copy the `github` object, paste it under our `mcpServers`:

```jsonc
// Us: ~/.local-agent/mcp.json
{
  "$schema": "https://local-agent.dev/schemas/mcp.json",
  "version": 1,
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "@modelcontextprotocol/server-github"],
      "env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" }
    }
  }
}
```

(While pasting, swap inlined secrets for `${VAR}` references — see [env-vars.md](env-vars.md).)

## Our extensions

We add two things on top of the Claude Desktop format:

1. **`enabled` field** per server — our extension; other editors ignore unknown fields. `false` filters a server out before sending to ACP without deleting its config. See [config.md § enabled](config.md#the-enabled-field).
2. **Envelope** (`$schema`, `version`) around the `mcpServers` map — strictly a superset at the top level. The inner map stays portable.

If you round-trip our file through Claude Desktop, it drops `enabled`/`$schema`/`version` on save — which is fine. Re-enable servers in our UI afterward.

## VS Code (the outlier)

VS Code uses `servers` instead of `mcpServers` and adds `inputs` for `${input:api-key}` prompts, `envFile`, and `sandbox`. The per-server object shape is otherwise the same. To copy from VS Code to us:

1. Take the object under `servers` (not `mcpServers`).
2. Paste it under our `mcpServers`.
3. Replace any `${input:...}` references with `${VAR}` env references (we support `${VAR}` only, not `${input:...}`).

See [config.md](config.md) for our full format, [env-vars.md](env-vars.md) for `${VAR}` expansion, and [examples/](examples/) for drop-in configs.
