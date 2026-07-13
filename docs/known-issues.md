# Known Issues

Gaps and deferred work tracked from review passes. Each entry is a one-line
note so the next agent can pick it up without re-reading the full review file.

## Image upload — Claude Code inline-image delivery gap

The image upload flow (Mode B) is implemented per ACP spec: when an agent
advertises `PromptCapabilities.Image`, the transport sends an inline
`ImageBlock` (base64) with a `Uri` hint; otherwise it sends a
`ResourceLinkBlock` + text instruction telling the agent to read the file.

Claude Code CLI (as of 2.1.128) has a documented parity gap: it does not
reliably deliver inline base64 images to the model even when an `ImageBlock` is
passed in the stdin frame. The resource-link + "please read this file" fallback
path is therefore the robust one for Claude Code today — the agent reads the
file from disk via its own `read` tool (which is the path Claude Code actually
supports, however imperfectly). We mitigate the known `read`-tool bugs (mime
detection by extension, no validation) by validating magic bytes and writing
the correct extension ourselves before storing the upload.

When Claude Code fixes the inline-image gap upstream, no change is needed on
our side — the capability gate will already send the inline `ImageBlock` to any
agent that advertises image support.

## MCP-over-ACP (P4.10) — blocked on SDK `mcp/message` codegen

Investigated 2026-07-13. Deferred; the working inline MCP transport (client
passes stdio/http/sse configs to the agent) is retained.

`coder/acp-go-sdk` v0.13.5 (latest) code-generates `mcp/connect`
(`UnstableConnectMcp`) and `mcp/disconnect` (`UnstableDisconnectMcp`) but **not**
`mcp/message` — the bidirectional relay that carries the actual inner MCP
JSON-RPC. `ClientSideConnection.handle` doesn't dispatch inbound `mcp/message`
(returns `MethodNotFound`), there's no method to send `mcp/message` to the agent,
the `_`-prefixed extension escape-hatch doesn't cover it, and the underlying
`*Connection` is unexported. So a functional broker isn't possible without
forking the SDK or replacing our `ClientSideConnection` usage with a hand-rolled
`acp.NewConnection` layer — both disproportionate for an unstable protocol no
mainstream agent advertises (`mcp_capabilities.acp`) yet.

Fix path / unblock signal + full drop-in design: `docs/plans/acp-spec-compliance.md` § 4.10.

## Security audit — deferred findings (from 2026-07-07 audit)

- **sec-auth-credentials-in-query-params (Low):** Device credentials are passed
  as `deviceId`/`secret` query params on WebSocket/SSE handshakes (browsers
  can't set headers on WS). Acceptable trade-off for direct LAN+TLS, but a
  short-lived single-use WS ticket exchanged via the authenticated REST API
  would eliminate the leakage vector if a reverse proxy is ever placed in
  front.
