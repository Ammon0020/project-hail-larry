# ACP-compliant profile and MCP transition

> Security + UX follow-up. Difficulty: hard. Urgency: high.
> Source: `docs/reviews/2026-07-21/HANDOFF-tool-whitelist-on-profile-switch.md`.

## Goal

Let a user change profile while working without claiming that inline MCP access changed when ACP
cannot change it in place. Keep the stable ACP boundary: the MCP **server** list is set on the
new agent session. Do not introduce MCP-over-ACP, an MCP proxy, or
`agent-client-protocol-rmcp` here.

## Product behavior

When the selected profile would change the effective MCP server set, open a profile-transition
dialog rather than changing the current session immediately. Its copy is intentional:

> ### Tool access changes with a new session
>
> **{Profile}** changes both instructions and MCP server access. ACP sets MCP servers when an
> agent session starts, so this conversation’s server access cannot change in place.

Use three radio choices, followed by `Continue` and `Cancel` buttons:

1. **Start a new agent session with history** *(default)* —
   “Your conversation will be carried into the new session’s first prompt. The new profile’s
   instructions and MCP server access apply there.”
2. **Start a fresh conversation** —
   “Open a blank conversation with this profile. This conversation stays unchanged.”
3. **Apply instructions only** —
   “Use this profile’s instructions in the current conversation. Its existing MCP server access
   stays the same.”

`Cancel` leaves both instructions and server access unchanged. The dialog must name the target
profile and be keyboard accessible. After choice 3, show persistent, compact state near the
profile/tools controls: “Instructions: {target}; MCP servers: this session’s existing access.”
This prevents the selector from implying that server access changed.

No dialog is needed when the target profile has the same effective MCP server set, or no MCP
servers are attached; switch instructions through `session/set_config_option` as today.

## Stable ACP behavior

- **History**: rebind the local conversation to a fresh ACP agent session, using the existing
  durable transcript transfer for its first prompt. Keep the local conversation/event history.
  Close the old actor before starting the replacement; do not call a made-up live MCP rebind RPC.
- **Fresh**: create a new local and ACP session. The original session remains open and unchanged.
- **Instructions only**: retain the current `POST /sessions/:id/profile` path, but document and
  return that it changes instructions/mode only; it must not claim an MCP policy update.
- **Initial profile**: accept `profileId` during `POST /sessions`, validate and store it before
  actor startup, then derive `mcpServers` from that profile. Applying it after session creation is
  unsafe because the actor has already received the default profile's MCP configuration.

## MCP server policy

Make the profile policy an explicit server allowlist, not a per-tool allowlist:

```json
{ "mcpServers": ["workspace-read", "context7"] }
```

Missing `mcpServers` preserves backward-compatible “all enabled servers” behavior; an explicit
empty list means “no MCP servers.” Validate every name against `mcp.json` on profile save and
surface unavailable/disabled servers in Settings.

This model works well when server boundaries match capabilities: for example,
`workspace-read` and `workspace-write` may be selected independently alongside `context7`. It
cannot split a server that itself exposes both read and write tools. Such a server must be made
read-only/upstream-restricted or be omitted from the restrictive profile.

This policy governs MCP integrations only. ACP's built-in client callbacks (such as file writes
or terminals) remain governed by the existing permission policy; do not market an MCP profile as
global “read-only” or “write access” unless that broader permission policy is explicitly added.

Migrate existing `tools` entries deliberately rather than silently treating them as server names.
Keep them read-only/legacy only long enough to present a Settings migration UI, then remove the
tool enumeration/filtering path once no other feature consumes it. Do not implement a custom
per-tool broker until it is a deliberate product requirement and stable ACP support exists.

## API shape

- Add a profile-transition operation with an explicit strategy: `history` or `fresh`.
  It validates the target profile, performs the lifecycle operation, and reports the resulting
  session id. `cancel` is client-only; `instructionsOnly` remains the existing profile endpoint.
- Add optional `profileId` to session creation and wire it through the frontend's selected profile.
- Preserve atomicity: on validation/startup/transfer failure, keep the current profile and actor
  usable; a fresh-session failure must not alter the original session.
- Treat the HTTP surface as a contract change and update contract coverage.

## Implementation slices

1. Backend profile schema migration: `mcpServers` allowlist, initial `profileId`, direct
   server-name filtering, and a profile-aware rebind/fresh-session operation.
2. HTTP/API contracts and focused Rust tests for state rollback, profile-before-startup, mixed
   server exclusion, history transfer, and fresh-session isolation.
3. `ProfileTransitionDialog` beside the existing `SwitchAgentDialog`; wire the selector, loading
   state, errors, focus restoration, mobile layout, and the persistent instructions-only notice.
4. Run `cargo test -q --all-targets`, `cargo clippy -q --all-targets -- -D warnings`,
   `cargo fmt --check -q`, `make test-contract`, `npm run lint --silent`, and
   `npm run build --silent` in `web/`.

## Acceptance

- A profile switch is always available; users can select history, fresh, instructions-only, or
  cancel when MCP server access differs.
- History starts a new ACP session with the target profile's server list and transfers the
  visible durable transcript exactly once on its first prompt.
- Fresh uses the target profile before actor startup; the original conversation stays unchanged.
- Instructions-only sends fresh profile instructions/mode but retains the exact prior MCP server
  configuration, with persistent disclosure.
- A profile can allow `workspace-write` and `context7` while another allows only
  `workspace-read`; a mixed read/write server is never represented as partial access.
- No implementation depends on `unstable_mcp_over_acp`, ACP extensions, or an MCP proxy.

## Handoff

Suggested commit: `feat(profiles): offer ACP-safe MCP transition choices`
