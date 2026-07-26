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


**Severity:** Medium — user-facing annoyance on every restart, no security
risk (auth still required each time).

**Fix path / unblock signal:** One of (a) an ACP SDK change exposing
persistable auth state (tokens/cookies in `AuthenticateResponse`), (b) a
Mistral Vibe change to persist auth locally and skip the ACP authenticate
handshake when valid, or (c) a daemon-side approach that keeps auth alive
across restarts (not possible while tokens are opaque). Track upstream SDK
releases for (a).

## Profile-over-ACP fallback branch — tested 2026-07-25 (was Medium)

Resolved 2026-07-25. The prompt-injection fallback in
`src/acp/providers.rs` (`find_profile_config_id == None` path, triggered when
an agent does NOT advertise the `mode`-category `profile` config option) is now
covered by two tests:

- `profile_is_injected_when_the_agent_lacks_profile_configuration` in
  `src/acp/context.rs` — unit-level: asserts profile instructions are injected
  as a `Profile Instructions` resource (embedded path) and as a
  `## Active Profile:` text section (text-fallback path) when
  `include_profile=true`.
- `prompt_injection_fallback_skips_set_config_option` in
  `src/acp/core/lifecycle/tests.rs` — end-to-end: uses the `mock-nocap` agent
  (registered in `mock_client_empty` via the `env MOCKAGENT_NO_MODE_CAP=1`
  wrapper), sends a prompt, and asserts the streamed reply contains no
  `[profile:` marker (proving `session/set_config_option` was skipped) while
  `session_for_profile_switch` reports `None`.

No `AgentInfo` schema change was needed: `mock_client_empty` already works
around the per-test env-var limitation by using `env` as the agent command and
passing `MOCKAGENT_NO_MODE_CAP=1` as an arg. The earlier note about a schema
change being required was incorrect — the `env` wrapper avoids the
process-global `set_var` race without touching `AgentInfo`.

The capability-present branch remains covered by
`mockagent_initial_profile_sent_over_acp_when_capability_advertised`.

## Clippy pedantic — resolved 2026-07-25 (was Low)

Resolved 2026-07-25. The `[lints.clippy]` table in `Cargo.toml` now denies
`clippy::pedantic` across lib AND test targets. All 434 pedantic findings
surfaced by `cargo clippy --all-targets -- -W clippy::pedantic` were cleared
in a single pass:

- 174 auto-fixed by `cargo clippy --fix` (redundant_closure_for_method_calls,
  map_unwrap_or, unnested_or_patterns, semicolon_if_nothing_returned,
  needless_raw_string_hashes, simple assigning_clones, etc.).
- 59 `assigning_clones` applied by hand in `src/interfaces/wire.rs`,
  `src/api/mod.rs`, `src/permissions/*` (`x = y.clone()` →
  `x.clone_from(&y)`); `--fix` could not rewrite these cross-statement
  cases safely.
- 84 `missing_errors_doc` — `# Errors` sections added to all non-test
  public `Result`-returning fns (test modules had no findings).
- 40 Tier 4 fixes: `manual_let_else`, `items_after_statements`,
  `unused_async`, `trivially_copy_pass_by_ref`, `match_same_arms`,
  `format_push_string`, `format_collect`, `needless_pass_by_value`,
  `unused_self`, `needless_continue`.
- 40 Tier 5 judgment calls: scoped `#[allow]` with explanatory comments
  for `cast_*` (with safety invariants), `too_many_lines` (actor state
  machines / linear test sequences), `struct_excessive_bools` (independent
  config knobs), `similar_names` (intentional pid/ppid, http/https pairs),
  `missing_fields_in_debug` (secrets intentionally omitted),
  `match_wild_err_arm` (timeout paths).
- 37 stragglers in files not covered by the first pass (fswatch, pathutil,
  acp/stream, events/publisher, search/*, mcp, pairing).

Scoped `#[allow]` attributes remain in-tree, each with a comment
explaining why the lint does not apply (serde `skip_serializing_if`
contracts, cross-file call sites, actor state-machine flow clarity). These
are intentional and documented; future pedantic findings will fail CI.

Run to verify: `cargo clippy --all-targets -- -D warnings` (clean).

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
