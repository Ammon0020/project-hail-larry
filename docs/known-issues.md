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

## Profile-over-ACP fallback branch untested (Medium)

Investigated 2026-07-21. The prompt-injection fallback in
`src/acp/providers.rs` (`find_profile_config_id == None` path, triggered when
an agent does NOT advertise the `mode`-category `profile` config option) has no
test. The mockagent supports `MOCKAGENT_NO_MODE_CAP=1`
(`cmd/mockagent/main.go:37,129-131`) to suppress the advertisement, but the
test harness in `tests/acp_core_lifecycle.rs` spawns the mockagent via
`AgentInfo { command, args, ... }` with no per-test env-var field, and
`std::process::Command::new(&config.agent.command)` in `src/acp/core.rs` does
not thread env vars from the registry. Setting the env var process-wide would
race with the parallel capability-present tests (`cargo test` runs them
concurrently). Adding per-test env wiring requires an `AgentInfo` schema change
in `src/`, out of scope for the review pass.

The capability-present branch IS covered by
`mockagent_initial_profile_sent_over_acp_when_capability_advertised`. Story
S-PROF-ACP acceptance criterion #2 is marked `[~]` (partial) in
`docs/plans/profiles-over-acp/done-acp-set-config-option-send-hard.md`.

**Fix path:** Add an `env: Vec<(String, String)>` (or `HashMap`) field to
`AgentInfo` (`src/config/...`), thread it through `AgentRegistry` → actor spawn
in `src/acp/core.rs` (`std::process::Command::new(...).envs(...)`), then add a
test that registers a mockagent entry with `MOCKAGENT_NO_MODE_CAP=1`, creates a
session, sends a prompt, and asserts the reply does NOT start with
`[profile: code]` (proving `set_config_option` was skipped) while profile
instructions are still injected.

## Clippy 1.92.0 — `manual_inspect` lint in `src/acp/core.rs` (Low)

Investigated 2026-07-21. A toolchain bump to rust-1.92.0 introduced the
`clippy::manual_inspect` lint, which fires at `src/acp/core.rs:1422` (a
`map_err` closure that only logs and rethrows). Pre-existing on `main`;
unrelated to model-autodetection work. Fix: replace `map_err` with
`inspect_err` per the clippy suggestion. Out of scope for the current task.

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

## Daemon port-orphan recovery — non-Linux PID-from-port deferred

Fixed 2026-07-18 on Linux. `start` now probes the configured HTTP port before
binding and fails fast with the holding PID when an orphan holds it; `stop`
falls back to finding a process listening on the port when no live PID file
exists. macOS/Windows `find_pid_listening_on` return `Ok(None)` (no cheap
kernel introspection path is wired yet); on those platforms an orphaned daemon
without a PID file still requires a manual `kill`/`taskkill`. See
`docs/plans/other_tasks/active-daemon-port-orphan-recovery-small-high.md`.

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
