# Contract Fixtures (S-CONTRACT)

Golden fixtures that pin the external API contract for the Rust daemon. The
fixtures were originally captured from the Go daemon (now removed) and are
checked in as the byte-stable, redacted shape the Rust backend must reproduce.
They are the contract surface — the runner replays each one against the running
Rust daemon and compares responses.

## Layout

```
tests/contract/
  fixtures/seed-workspace/ Deterministic workspace the harness registers before
                           capture (README.md, src/greet.txt)
  golden/                 Checked-in fixtures — the contract surface
    rest/                 One JSON envelope per REST case (<name>.json)
    ws/                   WebSocket frame logs (.jsonl) + rejection envelopes (.json)
    dto/                  Shared DTO serialization shapes (<type>.json)
    cli/                  CLI command envelopes (<command>.txt)
  scripts/                Helper scripts
```

The golden fixtures are static and checked in; the runner compares against them
as-is. There is no live regeneration step — the Go harness that originally
captured them has been removed.

## What is captured

### REST (`golden/rest/<name>.json`)

Every supported REST route is exercised in-process via
`httptest.NewRequest` + `httptest.NewRecorder` so `RemoteAddr` can be
controlled. Each route has at least a success case (loopback, auth bypass) and
a relevant failure case (non-loopback unauthenticated → 401, bad body → 400,
cross-origin mutating → 403, not-found → 404). The envelope is:

```json
{
  "method": "GET",
  "path":   "/health",
  "status": 200,
  "contentType": "application/json",
  "body":   "<redacted raw body string>"
}
```

The body is the **raw** response body (redacted), kept as text so the runner
can choose semantic or exact comparison per route.

### WebSocket (`golden/ws/`)

- `ws_auth_success.jsonl`, `ws_event_broadcast.jsonl` — real `nhooyr.io/websocket`
  client over the httptest server; the harness drives `nBroadcasts` synthetic
  broadcasts via `server.OnEvent` and records each frame as a JSONL line:
  `{"dir":"send","note":"..."}` for what the harness drove, `{"dir":"recv",
  "type":"text","event":{...}}` for what the client received.
- `ws_keepalive.jsonl` — documents the ping/timeout policy (pings are
  protocol-level control frames, not data frames, so they do not appear as
  JSONL messages).
- `ws_auth_rejection.json`, `ws_origin_rejection.json` — handler-level
  (pre-upgrade) rejection envelopes (HTTP status + body), same shape as a REST
  fixture.

### DTO (`golden/dto/<type>.json`)

Representative values of every shared DTO (`config.Config`, `interfaces.Event`
full + minimal, `WorkspaceInfo`, `SessionInfo`, `acp.AgentInfo` full + empty
optional, `pairing.PairingSession`, `DeviceCredential`, `DeviceInfo`,
`PendingActionInfo`, `FileNode` folder + file, `ProviderInfo` enabled +
disabled, `Attachment`) marshaled with `json.MarshalIndent`. Values are
constructed to exercise `omitempty` so the Rust side can see which fields are
dropped when empty. A fixed timestamp (`2026-07-13T12:00:00Z`) keeps time
fields byte-stable. (DTO type names refer to the original Go packages; the
Rust backend reproduces the same JSON shapes.)

### CLI (`golden/cli/<command>.txt`)

Each CLI command was originally captured as a subprocess of the real `app`
binary built from the (now-removed) `cmd/app`, with `LOCAL_AGENT_STATE_DIR`
pointing at the harness's isolated state dir. The httptest server was left
running and the harness rewrote `config.json` with the server's port and wrote
a `daemon.pid` file pointing at the harness process, so commands that talk to
the daemon (`pair`, `devices`, `revoke`, `logs`) hit the in-process server. A
device was paired through the live server first so `devices` and `revoke` had
a real target. These fixtures are now static documentation of the CLI envelope
shape; the runner does not exercise them.

Envelope format:

```
$ app <args...>
exit: <code>
--- stdout ---
<stdout>
--- stderr ---
<stderr>
```

Commands that would block (`start`) or modify the host system
(`install-service`, `uninstall-service`) were captured as their `--help` output,
which is itself a stable contract surface. `stop` was captured against a missing
PID file so it produces the not-running error instead of SIGTERMing the
harness.

## Redaction policy

All captured text passes through `Redactor` (originally `go-fixtures/redact.go`,
now ported to `tests/contract_runner/redactor.rs`) before it is written.
Redaction is **comparison-neutral**: the runner applies the same redactions to
its own output before comparing.

1. **Registered secrets** — pairing tokens, passcodes, device secrets collected
   during the run (e.g. the token returned by `/api/pair/initiate`) are
   replaced with `<REDACTED_TOKEN>`, `<REDACTED_PASSCODE>`, etc.
2. **Registered absolute path prefixes** — the isolated state dir and the
   user's home dir are replaced with `<REDACTED_PATH>`. Longer prefixes are
   matched first so a nested dir is scrubbed before its parent.
3. **Non-deterministic timestamps** — ISO-8601 timestamps emitted by
   `time.Time` JSON marshaling are replaced with `<REDACTED_TIMESTAMP>`.
4. **Long hex/base64 IDs** — tokens/IDs ≥ 20 hex chars are replaced with
   `<REDACTED_ID>`. Short IDs (e.g. 16-char workspace path hashes) are left
   intact so workspace-scoped routes remain readable; the seeded workspace ID
   is additionally registered when determinism matters.
5. **Defense-in-depth** — `ScrubUnregisteredTokens` rewrites any
   `"token"|"secret"|"secretHash"` JSON field with a ≥16-char value to
   `<REDACTED_TOKEN>`, in case a secret slips through without being
   registered.

## JSON comparison rules (for the Rust differential runner)

The runner reads each golden fixture, starts the Rust daemon with the same
isolated state dir + seed config, replays the same request sequence, applies
the same redactions, and compares:

- **REST bodies that are JSON objects/arrays** — semantic (parse both sides,
  compare structurally; field order is irrelevant). This covers the bulk of
  list/detail responses.
- **REST bodies that are contractually-significant text** — exact byte
  comparison. This covers error messages, markdown exports
  (`sessions_export_*`), and any non-JSON `Content-Type`.
- **REST envelope** (`method`, `path`, `status`, `contentType`) — exact.
- **DTO fixtures** — semantic JSON comparison (these exist specifically to
  pin field shapes and `omitempty` behavior, not key ordering).
- **WS JSONL** — line-by-line; each `recv` frame's `event` is compared
  semantically; `dir`/`type`/`note` are exact.
- **WS rejection envelopes** — exact (status + body).
- **CLI envelopes** — exact byte comparison of the full text envelope
  (stdout/stderr text is part of the contract).

## `LOCAL_AGENT_STATE_DIR`

The single override point consulted by `config.DefaultOrError` and
`config.Load` (originally `internal/config/config.go`; the Rust backend
respects the same env var). The harness sets it to a temp dir before
constructing the daemon and before spawning CLI subprocesses, so every manager
and every CLI invocation reads/writes inside that dir. This is what makes the
harness self-contained and is what the runner uses to isolate the Rust daemon
against the same seed config.

## Rust black-box differential runner (`tests/contract_runner/`)

The Rust runner is a `cargo test` integration test that boots the Rust backend
binary as a subprocess, replays the same request sequences captured by the
(original, now-removed Go) harness, applies the same redactions, and compares
responses against the checked-in golden fixtures. It is completely
backend-agnostic — it only interacts via the external API (HTTP, WebSocket, CLI
subprocess).

### Running

```sh
# Default — builds local_agent, boots it, runs all tests:
cargo test --test contract_runner --features contract -- --nocapture

# Use a pre-built binary instead of building:
CONTRACT_BINARY=/path/to/local_agent cargo test --test contract_runner --features contract

# Keep the state dir for debugging:
CONTRACT_KEEP_STATE=1 cargo test --test contract_runner --features contract
```

### What the runner tests

- **REST** (45 tests): every `golden/rest/*.json` fixture is replayed as an
  HTTP request. The redacted response is compared — semantic JSON for
  object/array bodies, exact bytes for error text and non-JSON content types.
  Envelope fields (method, path, status, contentType) are always compared
  exactly.
- **WebSocket** (5 tests): origin rejection (403), connection success (101 +
  ping/pong), live broadcast (pair+revoke → `DeviceRevocationPending`),
  `?after=` replay + live transition, and auth rejection (401 via non-loopback
  dial; harness binds `0.0.0.0`). Slow-client recovery is not black-box (see
  Known Limitations).
- **DTO** (3 tests): the JSON shapes from API responses are structurally
  compared against `golden/dto/*.json` fixtures to verify field names and
  omitempty behavior. The comparison is bidirectional with omitempty
  tolerance — fields that the API omits when empty are not required to be
  present.

CLI tests are intentionally excluded. The CLI is a thin client over the REST
API, and its output formatting (box-drawing, table layouts, help text) is
presentation, not contract. The REST + WS + DTO tests cover the actual API
contract surface that the Rust backend must replicate.

### Known limitations

- **`rest_agents_autodetect_smoke`** replaces the former `rest_agents_autodetect_ok`
  golden-fixture test, which was machine-specific (captured agents from the
  generation machine) and could not run in the neutralized black-box runner.
  The smoke test validates the endpoint contract (200, JSON array) without
  asserting on specific agent values.
- **`rest_mcp_put_bad_body` is active.** The golden fixture reflects the Rust
  `serde_json` parse-error suffix (`key must be a string at line 1 column 2`)
  for the malformed body `{not json`. The Go backend that produced the earlier
  `encoding/json` string has been removed, so the case now matches the Rust
  backend directly.
- **WebSocket slow-client recovery is not tested black-box.** Filling the
  64-deep send buffer and observing durable resync requires hub-internal
  control. Unit coverage: `src/sync/tests.rs`
  (`lagged_resync_from_bus_on_full_buffer`). Documented in
  `golden/ws/ws_after_replay.jsonl`.
- **CLI tests are not included**. The CLI is a thin client over the REST API.
  Its output formatting (box-drawing, table layouts, help text) is
  presentation, not contract. The checked-in `golden/cli/` fixtures are
  historical documentation captured by the original Go harness; the runner
  doesn't test them.

### CI

Linux job `contract` in `.github/workflows/rust-ci.yml` (after `test`):

```sh
cargo build --bin local_agent --locked
CONTRACT_BACKEND=rust CONTRACT_BINARY=./target/debug/local_agent \
  cargo test -q --test contract_runner --features contract -- --test-threads=1
```

SPA embed stub matches other jobs. Goldens are checked in and treated as the
contract surface; there is no live regeneration step (the original Go harness
has been removed).


### Backend selection

- `CONTRACT_BACKEND=rust` (default): builds `target/debug/local_agent` (or
  `CONTRACT_BINARY` override) and runs `local_agent start`.
- `CONTRACT_BINARY=/path/to/binary`: overrides the binary path. The runner uses
  this directly without building.

(`CONTRACT_BACKEND=go` is no longer supported — the Go toolchain, `cmd/app`,
and `internal/` have been removed; the runner panics if it is set.)

The runner sets `LOCAL_AGENT_STATE_DIR` to an isolated temp dir, writes a seed
`config.toml` (same camelCase fields as the original Go `config.json`), and
starts the backend with `<binary> start`. It also sets `PATH=/dev/null` and
`HOME=/dev/null` so autodetect cannot pick up host agents. It polls `/health`
until the backend is ready (up to 30s), then runs the tests. On shutdown it
kills the subprocess and cleans up the temp dir (unless `CONTRACT_KEEP_STATE`
is set).
