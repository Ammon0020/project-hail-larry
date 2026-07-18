#!/usr/bin/env bash
#
# Builds the Local Agent Interface (frontend + Rust daemon).
#
# Primary binary: bin/local_agent (also installed via cargo when available).

set -euo pipefail

# Resolve the project root (directory containing this script) so the build
# works regardless of the caller's current directory.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$ROOT_DIR"

# Colored status output (falls back to plain text when stdout is not a TTY).
if [[ -t 1 ]]; then
    CYAN=$'\033[36m'; GREEN=$'\033[32m'; RESET=$'\033[0m'
else
    CYAN=""; GREEN=""; RESET=""
fi

# Fail loudly if required tooling is missing.
for tool in npm cargo; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "ERROR: '$tool' is not installed or not on PATH." >&2
        exit 1
    fi
done

echo "${CYAN}1. Building frontend...${RESET}"
(cd web && npm run build)

echo "${CYAN}2. Building Rust daemon (local_agent)...${RESET}"
# rust-embed bakes web/dist into the binary at compile time.
cargo build --release
mkdir -p bin
cp -f target/release/local_agent bin/local_agent

# Install onto the user cargo bin when present so `local_agent` is on PATH.
CARGO_BIN="${CARGO_HOME:-$HOME/.cargo}/bin"
if [[ -d "$CARGO_BIN" ]]; then
    cp -f bin/local_agent "$CARGO_BIN/local_agent"
    echo "  Installed: $CARGO_BIN/local_agent"
fi

echo "${GREEN}Build complete!${RESET}"
echo "  Rust daemon: bin/local_agent   (run: local_agent start)"
