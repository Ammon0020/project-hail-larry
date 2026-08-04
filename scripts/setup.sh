#!/usr/bin/env bash
#
# Verifies and installs prerequisites for building the Local Agent Interface
# (Node.js + npm frontend, Rust toolchain, frontend deps in web/node_modules).
#
# Usage:
#   ./scripts/setup.sh           # verify + auto-install what's fixable (node_modules)
#   ./scripts/setup.sh --verify  # verify only; never install, exit non-zero on any gap

set -euo pipefail

# setup.sh lives in scripts/, so the project root is one level up.
ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

# Colored status output (falls back to plain text when stdout is not a TTY),
# mirroring the pattern used by build.sh.
if [[ -t 1 ]]; then
    CYAN=$'\033[36m'; GREEN=$'\033[32m'; RED=$'\033[31m'; RESET=$'\033[0m'
else
    CYAN=""; GREEN=""; RED=""; RESET=""
fi

VERIFY_ONLY=0
if [[ "${1:-}" == "--verify" ]]; then
    VERIFY_ONLY=1
fi

# Required versions. The Rust channel is read from rust-toolchain.toml when
# present (pinned toolchain), otherwise defaults to 1.92.0.
REQUIRED_NODE_MAJOR=20
REQUIRED_RUST="1.92.0"
if [[ -f "$ROOT_DIR/rust-toolchain.toml" ]]; then
    # Extract the channel = "x.y.z" value; fall back to the default on parse failure.
    PARSED_CHANNEL="$(grep -E '^[[:space:]]*channel[[:space:]]*=' "$ROOT_DIR/rust-toolchain.toml" \
        | head -n1 | sed -E 's/.*"([^"]+)".*/\1/' || true)"
    if [[ -n "$PARSED_CHANNEL" ]]; then
        REQUIRED_RUST="$PARSED_CHANNEL"
    fi
fi

# version_ge <a_major.minor> <b_major.minor>
# Returns 0 (true) if a >= b, 1 (false) otherwise. Compares major then minor
# numerically, so "20.11" >= "20.0" and "1.92" >= "1.92".
version_ge() {
    local a="$1" b="$2"
    local a_major a_minor b_major b_minor
    IFS='.' read -r a_major a_minor <<< "$a"
    IFS='.' read -r b_major b_minor <<< "$b"
    a_major="${a_major:-0}"; a_minor="${a_minor:-0}"
    b_major="${b_major:-0}"; b_minor="${b_minor:-0}"
    if (( a_major > b_major )); then return 0; fi
    if (( a_major < b_major )); then return 1; fi
    if (( a_minor >= b_minor )); then return 0; fi
    return 1
}

FAILURES=0

# fail <message...> — print a red ERROR block to stderr and bump the failure count.
fail() {
    echo "${RED}$1${RESET}" >&2
    shift
    while [[ $# -gt 0 ]]; do
        echo "$1" >&2
        shift
    done
    FAILURES=$((FAILURES + 1))
}

# --- Node.js -----------------------------------------------------------------
if ! command -v node >/dev/null 2>&1; then
    fail "ERROR: 'node' is not installed or not on PATH." \
         "  Required: Node.js ${REQUIRED_NODE_MAJOR} or newer." \
         "  Install: https://nodejs.org/ or via nvm (https://github.com/nvm-sh/nvm)"
else
    # `node --version` prints e.g. "v20.11.0"; strip the leading 'v'.
    NODE_RAW="$(node --version)"
    NODE_VER="${NODE_RAW#v}"
    NODE_MAJOR="$(printf '%s' "$NODE_VER" | cut -d. -f1)"
    if (( NODE_MAJOR < REQUIRED_NODE_MAJOR )); then
        fail "ERROR: node version ${NODE_VER} is older than the required ${REQUIRED_NODE_MAJOR}." \
             "  Upgrade: https://nodejs.org/ or \`nvm install ${REQUIRED_NODE_MAJOR}\`"
    fi
fi

# --- npm ---------------------------------------------------------------------
if ! command -v npm >/dev/null 2>&1; then
    fail "ERROR: 'npm' is not installed or not on PATH." \
         "  Required: npm (ships with Node.js ${REQUIRED_NODE_MAJOR}+)." \
         "  Install: https://nodejs.org/ or via nvm (https://github.com/nvm-sh/nvm)"
fi

# --- cargo -------------------------------------------------------------------
if ! command -v cargo >/dev/null 2>&1; then
    fail "ERROR: 'cargo' is not installed or not on PATH." \
         "  Required: Rust toolchain (channel ${REQUIRED_RUST})." \
         "  Install: https://www.rust-lang.org/tools/install (rustup)"
fi

# --- rustc (version check) ---------------------------------------------------
if ! command -v rustc >/dev/null 2>&1; then
    fail "ERROR: 'rustc' is not installed or not on PATH." \
         "  Required: rustc >= ${REQUIRED_RUST}." \
         "  Install: https://www.rust-lang.org/tools/install (rustup)"
else
    # `rustc --version` prints e.g. "rustc 1.92.0 (...)"; take the 2nd field,
    # then reduce to major.minor for the version_ge comparison.
    RUSTC_VER="$(awk '{print $2}' <<< "$(rustc --version)")"
    RUSTC_MM="$(cut -d. -f1,2 <<< "$RUSTC_VER")"
    REQUIRED_RUST_MM="$(cut -d. -f1,2 <<< "$REQUIRED_RUST")"
    if ! version_ge "$RUSTC_MM" "$REQUIRED_RUST_MM"; then
        fail "ERROR: rustc version ${RUSTC_VER} is older than the required ${REQUIRED_RUST}." \
             "  Upgrade: \`rustup update\` or https://www.rust-lang.org/tools/install"
    fi
fi

# --- sccache -----------------------------------------------------------------
if ! command -v sccache >/dev/null 2>&1; then
    if [[ "$VERIFY_ONLY" -eq 1 ]]; then
        fail "ERROR: 'sccache' is not installed or not on PATH." \
             "  Required: sccache for build and test caching." \
             "  Install: \`cargo install sccache\` or via package manager (e.g., \`apt install sccache\` / \`brew install sccache\`)"
    else
        if command -v cargo >/dev/null 2>&1; then
            echo "${CYAN}Installing sccache via cargo...${RESET}"
            cargo install sccache --quiet || fail "ERROR: Failed to install sccache via cargo."
        fi
    fi
fi

# --- mold + clang (Linux x86_64 only) ----------------------------------------
# .cargo/config.toml wires mold as the linker via clang's -fuse-ld=mold for
# fat-LTO release builds. Mold is 2–5x faster than GNU ld and significantly
# offsets the link-time cost of whole-program optimization. On non-x86_64
# Linux or other platforms, .cargo/config.toml does not activate mold, so
# this check is scoped to x86_64 Linux only.
if [[ "$(uname -s)" == "Linux" && "$(uname -m)" == "x86_64" ]]; then
    if ! command -v mold >/dev/null 2>&1; then
        if [[ "$VERIFY_ONLY" -eq 1 ]]; then
            fail "ERROR: 'mold' is not installed or not on PATH." \
                 "  Required: mold linker for fast LTO release builds on x86_64 Linux." \
                 "  Install: \`sudo apt install mold\` or \`brew install mold\`"
        else
            echo "${CYAN}mold is not installed — release builds will use the default linker.${RESET}" >&2
            echo "${CYAN}For 2–5x faster linking, install mold: \`sudo apt install mold\`${RESET}" >&2
        fi
    fi
    if ! command -v clang >/dev/null 2>&1; then
        if [[ "$VERIFY_ONLY" -eq 1 ]]; then
            fail "ERROR: 'clang' is not installed or not on PATH." \
                 "  Required: clang linker driver (passes -fuse-ld=mold to the linker)." \
                 "  Install: \`sudo apt install clang\`"
        else
            echo "${CYAN}clang is not installed — required as the linker driver for mold.${RESET}" >&2
        fi
    fi
fi

# --- web/node_modules --------------------------------------------------------
NODE_MODULES_DIR="$ROOT_DIR/web/node_modules"
if [[ ! -d "$NODE_MODULES_DIR" ]]; then
    if [[ "$VERIFY_ONLY" -eq 1 ]]; then
        fail "ERROR: web/node_modules is missing — frontend dependencies are not installed." \
             "  Run: ./scripts/setup.sh   (or: cd web && npm ci)"
    else
        # Auto-fixable: install frontend deps. Requires npm (checked above); if npm
        # was missing the fail() above already recorded it, so guard before running.
        if command -v npm >/dev/null 2>&1; then
            echo "${CYAN}Installing frontend dependencies (npm ci)...${RESET}"
            (cd web && npm ci)
        else
            fail "ERROR: web/node_modules is missing and npm is unavailable to install them." \
                 "  Run: ./scripts/setup.sh   (or: cd web && npm ci)"
        fi
    fi
fi

# --- Exit handling -----------------------------------------------------------
if [[ "$FAILURES" -gt 0 ]]; then
    if [[ "$VERIFY_ONLY" -eq 0 ]]; then
        echo "${RED}Setup incomplete: ${FAILURES} issue(s) above could not be auto-fixed.${RESET}" >&2
    fi
    exit 1
fi

if [[ "$VERIFY_ONLY" -eq 1 ]]; then
    echo "${GREEN}Prerequisites OK${RESET}"
else
    echo "${GREEN}Setup complete!${RESET}"
fi
