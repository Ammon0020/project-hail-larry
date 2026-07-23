# `agent-client-protocol-rmcp` migration superseded

> Chore. Difficulty: small. Urgency: low. Completed: 2026-07-22.

## Resolution

The planned migration only existed to replace the MCP `tools/list` catalog.
Profiles now allow complete MCP servers, which is the stable ACP model, so the
catalog was removed instead. `agent-client-protocol-rmcp` is an adapter for
building RMCP-backed MCP servers/proxies; it is not a drop-in MCP client for
the removed code.

Do not add the dependency speculatively. Reassess it only if the product later
chooses to build an explicit MCP broker or server, with the ACP/MCP boundary and
upstream stability reviewed at that time.
