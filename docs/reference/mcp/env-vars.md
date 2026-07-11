# MCP Environment Variable Expansion (`${VAR}`)

How `${VAR}` references work in `env` and `headers`. For the config format see [config.md](config.md).

## The rule

Values like `"${GITHUB_TOKEN}"` are expanded against `os.Getenv` **at session start** — not at save time. The literal `${VAR}` string is what's stored on disk; expansion happens only when the daemon builds the ACP `McpServer` for `session/new`, `session/load`, or `session/resume`.

```json
{
  "github": {
    "command": "npx",
    "args": ["-y", "@modelcontextprotocol/server-github"],
    "env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" }
  }
}
```

At session start, `${GITHUB_TOKEN}` is replaced with the value of the `GITHUB_TOKEN` environment variable on the host daemon. The expanded value is sent to the agent over the wire; it never lives in the config file.

## Why reference, not inline

Secrets should be **referenced, not inlined**. Keeping `${VAR}` in the file:

- Keeps API keys and tokens out of `mcp.json` so the file can be shared, screenshotted, or committed without leaking secrets.
- Means a `cp friend/mcp.json ~/.local-agent/mcp.json` doesn't hand over your friend's tokens — each user's env supplies their own.
- The file is mode `0600`, but referenced secrets are still safer than committed ones.

| Do | Don't |
|---|---|
| `"env": { "GITHUB_TOKEN": "${GITHUB_TOKEN}" }` | `"env": { "GITHUB_TOKEN": "ghp_xxxxxxxxxxxx" }` |
| `"headers": { "Authorization": "Bearer ${LINEAR_TOKEN}" }` | `"headers": { "Authorization": "Bearer lin_oauth_xxxxx" }` |

## When expansion happens

| Time | What's stored / sent |
|---|---|
| **Save** (`PUT /api/mcp`) | The literal `${VAR}` string is written to disk. No expansion. |
| **Session start** (`session/new` / `load` / `resume`) | `${VAR}` is expanded via `os.Getenv` when building the ACP `McpServer`. The expanded value goes on the wire. |

This means changing an environment variable on the host takes effect on the **next** session start — not for the currently running session. See [config.md § Limitations](config.md#limitations).

## Unset variables

If `${VAR}` references a variable that is unset at session start, the literal `${VAR}` string is passed through unchanged. The MCP server then receives `${VAR}` as its env value and will fail with its own error (e.g. auth failure) — which surfaces in the chat panel during `session/new`.

This is intentional: a missing secret shouldn't brick the whole session. The server that needs it fails clearly; other servers keep working.

## Where it applies

`${VAR}` expansion works in both `env` values (stdio) and `headers` values (http/sse):

```json
{
  "linear": {
    "type": "http",
    "url": "https://mcp.linear.app/mcp",
    "headers": { "Authorization": "Bearer ${LINEAR_TOKEN}" }
  }
}
```

Keys are never expanded — only values. `"${GITHUB_TOKEN}": "..."` would look for a variable named `${GITHUB_TOKEN}` as a key, which is not what you want.
