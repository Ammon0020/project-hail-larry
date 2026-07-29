- name: ProfilesSettings legacy-tools warning has no clear action — user can't remove them directly
- file: /media/adam/extex/projects/project-hail-larry/web/src/components/ProfilesSettings.tsx
- lines: 497-498, 677-682
- description: `updateServers` (line 497-498) sets `legacyTools: undefined` whenever the
  user changes the MCP server selection (toggle a server, or toggle the "All enabled MCP
  servers" checkbox). The warning at line 677-682 says "Legacy tool names (…) were not
  converted to server names. Choose MCP servers above before saving this profile." But
  there's no explicit "Clear legacy tools" button, and if the user wants to keep
  `mcpServers === undefined` (all servers) AND clear `legacyTools`, they must toggle the
  "All enabled MCP servers" checkbox off and back on — an undiscoverable gesture. The
  warning text implies selecting servers is required, but selecting "All enabled" (the
  default state) also clears it via the checkbox toggle. On save, `legacyTools` is sent
  as-is if untouched, and the backend validates each name (`profile_config.rs:268-270`)
  but doesn't reject them — so they persist silently. Fix: add a small "Clear legacy
  entries" link/button next to the warning that calls `updateServers(entry.mcpServers)`
  (which sets `legacyTools: undefined` while preserving the current server selection).
- verification: Read `ProfilesSettings.tsx:497-498` (updateServers clears legacyTools),
  `636-641` (the "All enabled" checkbox calls `updateServers(undefined)` only on change),
  `677-682` (warning with no action). Confirmed no button to clear legacyTools without
  changing the server selection.
