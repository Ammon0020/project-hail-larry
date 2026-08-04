# Refresh profile MCP server choices after configuration changes

> Chore. Difficulty: small. Urgency: low.

## Goal

Keep the Profiles Settings checkbox list in sync when `mcp.json` is saved or a
server is enabled/disabled without requiring the user to close and reopen
Settings.

## Scope

- Notify the mounted Profiles Settings view after successful MCP save/toggle,
  then refetch `/api/mcp` and rebuild its server choices.
- Preserve explicitly selected disabled and removed server names as unavailable
  choices so a user can remove them from a profile.
- Do not change the profile policy: omitted `mcpServers` continues to mean all
  servers enabled when a future agent session starts.

## Verification

- Component test or focused manual check for save/toggle refresh and stale
  selection retention.
- Frontend lint and production build.

Suggested commit: `fix(profiles): refresh MCP server choices after config changes`
