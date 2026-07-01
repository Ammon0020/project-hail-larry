#!/usr/bin/env bash
#
# Builds the Local Agent Interface project (Frontend + Backend) on Linux/macOS.
#
# This script builds the React frontend, copies the compiled assets into the
# Go server's embed directory, and then builds the final Go executable.
# It is the Unix counterpart to build.ps1.

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

# Go is commonly installed under /usr/local/go/bin but not always on PATH.
if ! command -v go >/dev/null 2>&1 && [[ -x /usr/local/go/bin/go ]]; then
    export PATH="$PATH:/usr/local/go/bin"
fi

# Fail loudly if required tooling is missing.
for tool in npm go; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "ERROR: '$tool' is not installed or not on PATH." >&2
        exit 1
    fi
done

echo "${CYAN}1. Building frontend...${RESET}"
(cd web && npm run build)

echo "${CYAN}2. Copying frontend assets to Go embed directory...${RESET}"
DIST_DIR="internal/server/dist"
mkdir -p "$DIST_DIR"
rm -rf "${DIST_DIR:?}"/*
cp -R web/dist/. "$DIST_DIR/"

echo "${CYAN}3. Building Go backend...${RESET}"
# Build into the bin folder.
go build -o bin/app ./cmd/app
# Also install it to GOBIN/GOPATH so 'app start' works globally.
go install ./cmd/app

echo "${GREEN}Build complete! The binary is available at bin/app and installed globally as 'app'.${RESET}"
