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

# Ensure cargo's user bin dir is on PATH. `cargo <subcommand>` finds
# extensions like cargo-watch via its own lookup of ~/.cargo/bin even when
# that dir isn't on PATH, but `command -v cargo-watch` below uses the shell's
# PATH. Prepend it so the detection matches cargo's behavior.
if [[ -d "${CARGO_HOME:-$HOME/.cargo}/bin" ]]; then
    case ":${PATH}:" in
        *":${CARGO_HOME:-$HOME/.cargo}/bin:"*) ;;
        *) export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH" ;;
    esac
fi

# build.rs requires web/dist/index.html for cargo run to compile. The release
# build leaves it in place; only `make clean` removes it. Fail fast with a
# clear message instead of a confusing build.rs error.
if [[ ! -f web/dist/index.html ]]; then
    echo "error: web/dist/index.html is missing." >&2
    echo "  build.rs requires it for 'cargo run' to compile." >&2
    echo "  Run once:  cd web && npm run build" >&2
    exit 1
fi

# Pre-flight: refuse to start if port 7337 is already bound. A stale daemon
# (e.g. from `local_agent start` left running) would cause the wait loop below
# to get a false-positive /health response, making Vite start before the new
# daemon is ready and producing ECONNREFUSED proxy errors.
if curl -sf http://127.0.0.1:7337/health >/dev/null 2>&1; then
    echo "error: port 7337 is already in use by a running daemon." >&2
    echo "  Stop it first, then re-run this script:" >&2
    echo "    pkill -f 'local_agent start'   # or" >&2
    echo "    fuser -k 7337/tcp" >&2
    exit 1
fi

# Pre-flight: refuse to start if port 5173 is already bound. A stale Vite from
# a previous dev.sh run that didn't clean up would keep the browser pinned to
# the old port (which proxies to a dead/missing daemon → "Reconnecting"
# forever). Vite is configured with strictPort, so it would fail anyway — but
# this gives a clearer message and the fix before Vite spews a stack trace.
if ss -tlnH 2>/dev/null | grep -q '127.0.0.1:5173\|:5173 '; then
    echo "error: port 5173 is already in use (likely a stale Vite from a" >&2
    echo "  previous dev.sh run that didn't clean up)." >&2
    echo "  Kill it, then re-run this script:" >&2
    echo "    fuser -k 5173/tcp   # or" >&2
    echo "    pkill -f 'vite'" >&2
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
    # Kill the whole process group for each dev process. `setsid` (below) puts
    # each process in its own session/process group, so `kill -- -PGID` reaps
    # the entire subprocess tree (npm→vite, cargo-watch→cargo→daemon). A bare
    # `kill $PID` only hits the parent and can orphan children — e.g. a
    # leftover Vite holding port 5173, which makes the next dev.sh run
    # silently move to 5174 and leaves the browser stuck on the stale 5173
    # proxy ("Reconnecting" forever).
    # Guard each kill with `|| true` because `set -e` would abort the trap
    # (and skip later kills) if the group is already gone.
    [[ -n "$DAEMON_PID" ]] && kill -- -"$DAEMON_PID" 2>/dev/null || true
    [[ -n "$VITE_PID" ]] && kill -- -"$VITE_PID" 2>/dev/null || true
    # Wait briefly for clean exit, then force-kill the groups if still alive.
    sleep 0.5
    [[ -n "$DAEMON_PID" ]] && kill -9 -- -"$DAEMON_PID" 2>/dev/null || true
    [[ -n "$VITE_PID" ]] && kill -9 -- -"$VITE_PID" 2>/dev/null || true
    wait 2>/dev/null || true
}
trap cleanup EXIT INT TERM

echo "${CYAN}Starting Rust daemon (cargo run -- start)...${RESET}"
# If cargo-watch is available, use it to auto-rebuild + restart on Rust source
# changes. Scope the watch to src/ and root build manifests so frontend changes
# under web/ (handled by Vite HMR) don't trigger redundant daemon rebuilds.
# Without cargo-watch, fall back to a single `cargo run -- start`.
# `setsid` puts each process in its own process group so cleanup can reap the
# whole subprocess tree (cargo-watch→cargo→daemon, npm→vite) via kill -- -PGID.
# Without `set -m`, setsid does NOT double-fork, so $! is both the PID and the
# PGID — kill -- -$! hits the whole group.
if command -v cargo-watch >/dev/null 2>&1; then
    echo "${CYAN}cargo-watch detected — daemon will auto-rebuild on Rust changes.${RESET}"
    setsid cargo watch \
        -w src -w Cargo.toml -w build.rs -w rust-toolchain.toml -w configs \
        -x 'run -- start' &
else
    echo "${YELLOW}cargo-watch not found — Rust changes require a manual restart.${RESET}"
    echo "${YELLOW}Install it for auto-rebuild: cargo install cargo-watch${RESET}"
    setsid cargo run -- start &
fi
DAEMON_PID=$!

# Wait for the daemon to bind its HTTP listener before starting Vite, so the
# proxy doesn't fail on the first browser request. Poll /health for up to 120s
# — a cold cargo-watch compile can take well over a minute, and the pre-flight
# check above guarantees any response is from OUR daemon, not a stale one.
echo "${CYAN}Waiting for daemon to bind...${RESET}"
DAEMON_READY=false
for _ in $(seq 1 240); do
    if curl -sf http://127.0.0.1:7337/health >/dev/null 2>&1; then
        DAEMON_READY=true
        echo "${CYAN}Daemon is ready.${RESET}"
        break
    fi
    sleep 0.5
done
if [[ "$DAEMON_READY" != true ]]; then
    echo "${RED}error: daemon did not bind within 120s.${RESET}" >&2
    echo "${RED}  Check the cargo output above for compile errors.${RESET}" >&2
    exit 1
fi

echo "${CYAN}Starting Vite dev server (npm run dev)...${RESET}"
# setsid creates a new process group; without `set -m` it does NOT double-fork,
# so $! is both the PID and the PGID. The inner bash -c handles the cd to web/
# before running npm. This ensures cleanup's `kill -- -$VITE_PID` reaps the
# entire npm→vite subprocess tree.
setsid bash -c 'cd web && npm run dev' &
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
