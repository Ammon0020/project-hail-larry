# Contract Fixtures (S-CONTRACT)

Golden fixtures captured **from the current Go daemon** so the future Rust port
can prove external equivalence via a differential test. The Go daemon is treated
as the compatibility oracle: every fixture here is the byte-stable, redacted
shape the Rust implementation must reproduce.

Story: `docs/plans/rust-port/stories/S-CONTRACT-compatibility.md`

## Layout

```
tests/contract/
  go-fixtures/            Go harness that captures the fixtures (see below)
  fixtures/seed-workspace/ Deterministic workspace the harness registers before
                           capture (README.md, src/greet.txt)
  golden/                 Checked-in fixtures — the contract surface
    rest/                 One JSON envelope per REST case (<name>.json)
    ws/                   WebSocket frame logs (.jsonl) + rejection envelopes (.json)
    dto/                  Shared DTO serialization shapes (<type>.json)
    cli/                  CLI command envelopes (<command>.txt)
  scripts/                Helper scripts
```

The harness resets `golden/{rest,ws,dto,cli}/` on every run so removed or renamed
routes do not leave stale fixtures.

## Regenerating fixtures

The harness is exposed both as a `main` and as a test, so it works in
sandboxes where `go run` is blocked but `go test` is allowed.

```sh
# Preferred (works in sandboxed CI):
go test ./tests/contract/go-fixtures/ -run TestGenerateFixtures

# Equivalent when go run is allowed:
go run ./tests/contract/go-fixtures/
go run ./tests/contract/go-fixtures/ -keep-state   # leave temp state dir for debugging
```

`TestGenerateFixtures` regenerates the golden tree in place and asserts each
subdirectory is non-empty. The checked-in fixtures are the contract surface;
regenerate after any intentional API/DTO/CLI change and commit the diff.

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

The body is the **raw** response body (redacted), kept as text so the future
Rust runner can choose semantic or exact comparison per route.

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
fields byte-stable.

### CLI (`golden/cli/<command>.txt`)

Each CLI command is run as a subprocess of the real `app` binary built from
`cmd/app`, with `LOCAL_AGENT_STATE_DIR` pointing at the harness's isolated
state dir. The httptest server is left running and the harness rewrites
`config.json` with the server's port and writes a `daemon.pid` file pointing at
the harness process, so commands that talk to the daemon (`pair`, `devices`,
`revoke`, `logs`) hit the in-process server. A device is paired through the
live server first so `devices` and `revoke` have a real target.

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
(`install-service`, `uninstall-service`) are captured as their `--help` output,
which is itself a stable contract surface. `stop` is captured against a missing
PID file so it produces the not-running error instead of SIGTERMing the
harness.

## Redaction policy

All captured text passes through `Redactor` (`go-fixtures/redact.go`) before it
is written. Redaction is **comparison-neutral**: the future Rust runner applies
the same redactions to its own output before comparing.

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

## JSON comparison rules (for the future Rust differential runner)

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
`config.Load` (`internal/config/config.go`). The harness sets it to a temp dir
before constructing the daemon and before spawning CLI subprocesses, so every
manager and every CLI invocation reads/writes inside that dir. This is what
makes the harness self-contained and is what the future Rust runner will use to
isolate the Rust daemon against the same seed config.

## What is NOT here yet

The Rust-side differential runner is a **future story**. This package only
captures and checks in the Go fixtures. When the Rust daemon exists, a sibling
runner will boot it with the same `LOCAL_AGENT_STATE_DIR` + seed config, replay
the captured sequences, and diff against `golden/`.
