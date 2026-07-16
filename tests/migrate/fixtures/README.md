# Migrate fixtures (S-MIGRATE)

Anonymized Go-created state trees used by `src/migrate` tests.

## `go-state/`

Hand-built to match Go on-disk formats (not secrets from a real install):

| Artifact | Source |
|----------|--------|
| `config.json` | Go `internal/config.Config` JSON tags |
| `local-agent.db` | Go `internal/events` schema + `eventPayload` JSON |
| `devices.json` | Go `pairing.storedDevice` (SHA-256 hashes only) |
| `conversations.json` | Go `acp.Session` metadata export |
| `mcp.json` | Go `mcp.File` envelope |
| `uploads/` | Go `uploads.Manager` layout (`<session>/<id>.ext`) |
| `tls/` | Cert directory placeholder (no real keys) |

Paths under `/home/fixture/` are redacted placeholders. Migration rewrites
`dataDir`/`dbPath`/`tlsCertDir` to the active `LOCAL_AGENT_STATE_DIR` / state dir
under test.

Regenerate the SQLite file by re-running the Python block in the S-MIGRATE
implementation notes, or rebuild via a temp `LOCAL_AGENT_STATE_DIR` + Go daemon
and copy+redact.
