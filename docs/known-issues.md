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
  front.

## Rust port — S-SHELL WIP does not compile (parallel task)

As of 2026-07-16, `src/shell/mod.rs` contains in-progress S-SHELL work that
does not compile: it declares a `pub struct Result` that shadows
`std::result::Result`, breaking `resolve_cwd` and `run_inner`, and references
`mod tests;` (`src/shell/tests.rs`) that is also WIP. This blocks `cargo
check`/`cargo test`/`cargo fmt` for the whole crate until S-SHELL is fixed.
S-UPLOADS was verified by temporarily restoring the committed shell stub via
`git show HEAD:src/shell/mod.rs`; the WIP file was preserved at
`/tmp/shell_mod_wip.rs` and restored afterward. S-SHELL should rename its
`Result` struct (e.g. `ShellResult`) or scope it to avoid the std collision.

## Rust port — S-BUILD native release CI deferred

Basic `cargo build`/`test` on macOS/Windows runners exists in
`.github/workflows/rust-ci.yml` (stub `web/dist/index.html` for `build.rs`).
Deferred: native **release artifact** builds with real frontend embed + SPA
smoke tests on Win/macOS (see S-BUILD story).
