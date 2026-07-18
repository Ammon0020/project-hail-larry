#!/usr/bin/env bash
#
# SPA smoke test for a release (or debug) local_agent binary.
#
# Starts the daemon with an isolated LOCAL_AGENT_STATE_DIR, probes /health and
# / (HTML), then stops cleanly. Never touches ~/.local-agent.
#
# Usage:
#   scripts/spa-smoke.sh [path/to/local_agent]
#
# Defaults to ./target/release/local_agent (or .exe under Git Bash on Windows).

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if [[ -n "${1:-}" ]]; then
  BINARY="$1"
elif [[ -f ./target/release/local_agent.exe ]]; then
  BINARY=./target/release/local_agent.exe
elif [[ -f ./target/release/local_agent ]]; then
  BINARY=./target/release/local_agent
else
  echo "ERROR: no binary given and target/release/local_agent[.exe] missing" >&2
  exit 1
fi

if [[ ! -f "$BINARY" ]]; then
  echo "ERROR: binary not found: $BINARY" >&2
  exit 1
fi

# Prefer python for a free port (available on GHA macOS/Windows/Linux).
pick_port() {
  if command -v python3 >/dev/null 2>&1; then
    python3 - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  else
    python - <<'PY'
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()
PY
  fi
}

# Convert a path for the native Windows binary when running under Git Bash.
to_native_path() {
  local p="$1"
  if command -v cygpath >/dev/null 2>&1; then
    cygpath -w "$p"
  else
    printf '%s' "$p"
  fi
}

STATE_DIR="$(mktemp -d "${TMPDIR:-/tmp}/local-agent-spa-smoke.XXXXXX")"
STATE_DIR_NATIVE="$(to_native_path "$STATE_DIR")"
# TOML string escaping: backslashes must be doubled for Windows paths.
STATE_DIR_TOML="${STATE_DIR_NATIVE//\\/\\\\}"
if command -v cygpath >/dev/null 2>&1; then
  DB_PATH_TOML="${STATE_DIR_TOML}\\\\local-agent.db"
else
  DB_PATH_TOML="${STATE_DIR_TOML}/local-agent.db"
fi
PORT="$(pick_port)"
export LOCAL_AGENT_STATE_DIR="$STATE_DIR_NATIVE"

cleanup() {
  LOCAL_AGENT_STATE_DIR="$STATE_DIR_NATIVE" "$BINARY" stop >/dev/null 2>&1 || true
  rm -rf "$STATE_DIR"
}
trap cleanup EXIT

mkdir -p "$STATE_DIR"
# Minimal seed: free port, loopback, TLS off (HTTP-only smoke; faster startup).
# camelCase keys match Config serde (rename_all = "camelCase").
cat >"$STATE_DIR/config.toml" <<EOF
port = ${PORT}
host = "127.0.0.1"
dataDir = "${STATE_DIR_TOML}"
dbPath = "${DB_PATH_TOML}"
tlsEnabled = false
pairingTtlSeconds = 300
EOF

echo "[spa-smoke] binary=${BINARY}"
echo "[spa-smoke] state_dir=${STATE_DIR_NATIVE}"
echo "[spa-smoke] port=${PORT}"

"$BINARY" start --background

BASE="http://127.0.0.1:${PORT}"
ready=0
for _ in $(seq 1 60); do
  if curl -fsS -m 2 "${BASE}/health" >/dev/null 2>&1; then
    ready=1
    break
  fi
  sleep 0.5
done

if [[ "$ready" -ne 1 ]]; then
  echo "ERROR: daemon did not become ready at ${BASE}/health within 30s" >&2
  echo "--- logs (if any) ---" >&2
  LOCAL_AGENT_STATE_DIR="$STATE_DIR_NATIVE" "$BINARY" logs 2>&1 | tail -n 80 >&2 || true
  exit 1
fi

HEALTH="$(curl -fsS -m 5 "${BASE}/health")"
echo "[spa-smoke] /health => ${HEALTH}"

BODY="$(curl -fsS -m 5 "${BASE}/")"
case "$BODY" in
  *'<!doctype html'*|*'<!DOCTYPE html'*|*'Local Agent'*)
    echo "[spa-smoke] / returned HTML (SPA embed OK)"
    ;;
  *)
    echo "ERROR: / did not look like embedded SPA HTML" >&2
    echo "body (first 400 chars): ${BODY:0:400}" >&2
    exit 1
    ;;
esac

"$BINARY" stop
trap 'rm -rf "$STATE_DIR"' EXIT

echo "[spa-smoke] OK"
