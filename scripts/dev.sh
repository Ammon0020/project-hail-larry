#!/usr/bin/env bash
#
# All-in-one dev startup: Rust daemon (cargo run) + Vite dev server (HMR).
#
# The Vite dev server proxies /api, /ws, and /health to the daemon (see
# web/vite.config.ts), so you open http://localhost:5173 in the browser and
# get instant frontend HMR.
#
# If `cargo-watch` is installed, the Rust daemon auto-rebuilds and restarts
# on changes under src/, Cargo.toml, build.rs, rust-toolchain.toml, and
# configs/. Otherwise the daemon is started once with `cargo run` and Rust
# changes require a manual restart (Ctrl+C, then re-run this script).
# Install cargo-watch with: `cargo install cargo-watch`.
#
# Usage:
#   scripts/dev.sh          # foreground, Ctrl+C kills both
#   make dev                # same via Makefile
#
# Prerequisites: web/dist/index.html must exist (build.rs requires it for
# `cargo run`). Run `cd web && npm run build` once if you've run `make clean`.

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# build.rs requires web/dist/index.html for cargo run to compile. The release
# build leaves it in place; only `make clean` removes it. Fail fast with a
# clear message instead of a confusing build.rs error.
if [[ ! -f web/dist/index.html ]]; then
    echo "error: web/dist/index.html is missing." >&2
    echo "  build.rs requires it for 'cargo run' to compile." >&2
    echo "  Run once:  cd web && npm run build" >&2
    exit 1
fi

# Colored prefixes for interleaved output (falls back to plain text when not a TTY).
if [[ -t 1 ]]; then
    CYAN=$'\033[36m'; YELLOW=$'\033[33m'; RED=$'\033[31m'; GREEN=$'\033[32m'; BOLD=$'\033[1m'; RESET=$'\033[0m'
else
    CYAN=""; YELLOW=""; RED=""; GREEN=""; BOLD=""; RESET=""
fi

DAEMON_PID=""
VITE_PID=""

cleanup() {
    echo ""
    echo "${YELLOW}Shutting down dev processes...${RESET}"
    [[ -n "$DAEMON_PID" ]] && kill "$DAEMON_PID" 2>/dev/null || true
    [[ -n "$VITE_PID" ]] && kill "$VITE_PID" 2>/dev/null || true
    # Wait briefly for clean exit, then force-kill if still alive.
    sleep 0.5
    [[ -n "$DAEMON_PID" ]] && kill -9 "$DAEMON_PID" 2>/dev/null || true
    [[ -n "$VITE_PID" ]] && kill -9 "$VITE_PID" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "${CYAN}Starting Rust daemon (cargo run -- start)...${RESET}"
# If cargo-watch is available, use it to auto-rebuild + restart on Rust source
# changes. Scope the watch to src/ and root build manifests so frontend changes
# under web/ (handled by Vite HMR) don't trigger redundant daemon rebuilds.
# Without cargo-watch, fall back to a single `cargo run -- start`.
if command -v cargo-watch >/dev/null 2>&1; then
    echo "${CYAN}cargo-watch detected — daemon will auto-rebuild on Rust changes.${RESET}"
    cargo watch \
        -w src -w Cargo.toml -w build.rs -w rust-toolchain.toml -w configs \
        -x 'run -- start' &
else
    echo "${YELLOW}cargo-watch not found — Rust changes require a manual restart.${RESET}"
    echo "${YELLOW}Install it for auto-rebuild: cargo install cargo-watch${RESET}"
    cargo run -- start &
fi
DAEMON_PID=$!

# Wait for the daemon to bind its HTTP listener before starting Vite, so the
# proxy doesn't fail on the first browser request. Poll /health for up to 15s.
echo "${CYAN}Waiting for daemon to bind...${RESET}"
for _ in $(seq 1 30); do
    if curl -sf http://127.0.0.1:7337/health >/dev/null 2>&1; then
        echo "${CYAN}Daemon is ready.${RESET}"
        break
    fi
    sleep 0.5
done

echo "${CYAN}Starting Vite dev server (npm run dev)...${RESET}"
( cd web && npm run dev ) &
VITE_PID=$!

echo ""
echo "${GREEN}${BOLD}╔══════════════════════════════════════════════════════════════╗${RESET}"
echo "${GREEN}${BOLD}║  Dev mode is running.                                        ║${RESET}"
echo "${GREEN}${BOLD}║                                                              ║${RESET}"
echo "${GREEN}${BOLD}║  ➜  Open this URL in your browser:                           ║${RESET}"
echo "${GREEN}${BOLD}║                                                              ║${RESET}"
echo "${GREEN}${BOLD}║     http://localhost:5173                                    ║${RESET}"
echo "${GREEN}${BOLD}║                                                              ║${RESET}"
echo "${GREEN}${BOLD}║  This is the Vite dev server with HMR.                       ║${RESET}"
echo "${GREEN}${BOLD}║  Do NOT open :7337 — that serves the old embedded build.    ║${RESET}"
if command -v cargo-watch >/dev/null 2>&1; then
    echo "${GREEN}${BOLD}║  Rust daemon auto-rebuilds on src/ changes (cargo-watch).    ║${RESET}"
else
    echo "${GREEN}${BOLD}║  Rust changes require a restart (no cargo-watch).            ║${RESET}"
fi
echo "${GREEN}${BOLD}║                                                              ║${RESET}"
echo "${GREEN}${BOLD}║  Daemon API (proxied):  http://localhost:7337                ║${RESET}"
echo "${GREEN}${BOLD}║  Press Ctrl+C to stop both.                                  ║${RESET}"
echo "${GREEN}${BOLD}╚══════════════════════════════════════════════════════════════╝${RESET}"
echo ""

# Wait for either process to exit. If one dies, cleanup kills the other.
# `wait -n` requires bash 4.3+; fall back to plain `wait` on older bash
# (macOS ships bash 3.2). The plain wait blocks until both exit, which is
# fine — the trap still fires on Ctrl+C.
if ! wait -n "$DAEMON_PID" "$VITE_PID" 2>/dev/null; then
    wait "$DAEMON_PID" "$VITE_PID" 2>/dev/null || true
fi
echo "${RED}A dev process exited; shutting down...${RESET}"
