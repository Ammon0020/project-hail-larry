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

## Mistral Vibe auth does not persist across daemon restarts (Medium)

Investigated 2026-07-12. The user reports Mistral Vibe asks for login every
server restart. Auth is browser-PKCE per-session in `internal/acp/acp.go`
(`startTransportLocked`, ~lines 478-493). The ACP SDK
(`coder/acp-go-sdk` v0.13.5) `AuthenticateResponse` only exposes
`Meta map[string]any` — no tokens, cookies, or auth state — so the auth state
is opaque inside the SDK's `ClientSideConnection` and is destroyed when the
daemon process restarts. `~/.vibe/.env` exists but is empty (no persisted
key); Mistral Vibe does not persist its own auth locally in a form the daemon
can reuse.

**Severity:** Medium — user-facing annoyance on every restart, no security
risk (auth still required each time).

**Fix path / unblock signal:** One of (a) an ACP SDK change exposing
persistable auth state (tokens/cookies in `AuthenticateResponse`), (b) a
Mistral Vibe change to persist auth locally and skip the ACP authenticate
handshake when valid, or (c) a daemon-side approach that keeps auth alive
across restarts (not possible while tokens are opaque). Track upstream SDK
releases for (a).

## Clippy pedantic — 4 findings to ratchet to deny (Low)

Investigated 2026-07-15. The `[lints.clippy]` table in `Cargo.toml` denies
`clippy::all` plus a curated set of restriction lints (no panics, no
`unwrap`/`expect`, no `dbg!`/`println!`/`eprintln!`, no `todo!`/`unimplemented!`)
in non-test code. The tree is clean under that policy.

Four `clippy::pedantic` findings exist today and are intentionally NOT denied
yet (denying them as `warn` would be escalated to errors by CI's blanket
`-D warnings`). Fix these, then add them to `[lints.clippy]` as `"deny"`:

- `src/lib.rs:12` — `doc_markdown`: item in documentation missing backticks
- `src/app/logging.rs:26` — `missing_errors_doc`: `Result`-returning fn lacks
  `# Errors` section
- `src/app/tls.rs:26` — `missing_errors_doc`: same
- `src/app/rate_limit.rs:38,52` — `must_use_candidate`: two fns could have
  `#[must_use]`

Run to verify: `cargo clippy --all-targets -- -A clippy::all -W clippy::pedantic`

## Security audit — deferred findings (from 2026-07-07 audit)

- **sec-auth-credentials-in-query-params (Low):** Device credentials are passed
  as `deviceId`/`secret` query params on WebSocket/SSE handshakes (browsers
  can't set headers on WS). Acceptable trade-off for direct LAN+TLS, but a
  short-lived single-use WS ticket exchanged via the authenticated REST API
  would eliminate the leakage vector if a reverse proxy is ever placed in
  front. Same pattern on `/raw` and `/preview` iframe URLs; preview responses
  send `Referrer-Policy: no-referrer` to limit Referer leakage.

- **sec-preview-same-origin-scripts (Medium — mitigated 2026-07-18):** Browse
  preview iframe no longer uses `allow-same-origin` (opaque origin; scripts
  run but cannot read IDE `localStorage`). Preview responses add
  `frame-ancestors 'self'`. Relative assets resolve correctly on loopback,
  where auth is bypassed, but a LAN browser does not propagate the entry
  URL's query credentials to relative subresource requests; preview now uses
  a 30-minute workspace-scoped in-memory ticket exchanged for an HttpOnly,
  path-scoped cookie. The residual risk is exposure of that short-lived ticket
  in the entry URL/server logs; `Referrer-Policy: no-referrer` limits onward
  Referer leakage. Workspace JS can still exfiltrate via third-party requests.

## Rust port — Go daemon deleted (cutover)

**Done 2026-07-18.** `cmd/app` and `internal/` removed. Remaining Go:
`cmd/mockagent` only (`go.mod` depends on `acp-go-sdk`). Re-run
`local_agent install-service` if systemd/launchd/HKCU still point at `app`.

## Rust port — story checkbox drift

Many `docs/plans/rust-port/*.md` ACs remain unchecked while modules and
tests already ship. Prefer mass check-off + a short “Remaining Rust gaps”
list over treating unchecked boxes as open work. Real open items live in
`docs/STATUS.md` Known Gaps.

## Contract harness — real `~/.local-agent/config.toml` overwrite risk

**Fixed 2026-07-18.** Root cause: `Daemon::new` unit tests in `src/app/daemon.rs`
built a `Config` with tempfile `data_dir` / `events.db` / `port=0` and called
`refresh_agents_on_startup` → `Config::save` **without** `LOCAL_AGENT_STATE_DIR`.
`save` writes to `resolved_state_dir()` (real `~/.local-agent`), not `data_dir`,
so autodetected agents + temp layout overwrote the host config (matched
`config.toml.*.bak`: `port=0`, `/tmp/.tmp…/events.db`, empty workspaces, real
agents). Mitigations: (1) `Config::save` refuses temp `data_dir` when the
active state dir is not under temp; (2) daemon tests set `LOCAL_AGENT_STATE_DIR`;
(3) contract harness uses a process-scoped fake `HOME` under the isolated state
dir instead of `HOME=/dev/null` (dirs passwd fallback). Regression:
`save_refuses_temp_data_dir_when_state_dir_is_not_temp`. Backups retained at
`~/.local-agent/config.toml.*.bak`.
