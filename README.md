# Local Agent Interface

A self-hosted web code editor with AI built in. A Rust daemon runs on your machine and serves a browser-based IDE to any device on your local network. You bring the AI agent (Claude Code, Gemini CLI, Codex, etc.) — the daemon orchestrates it alongside a VS Code-style editor. All files and state stay on your machine; connected devices are thin clients synced in real time.

## How It Works

1. **Register a workspace** — point the daemon at a project folder
2. **Start the daemon** — it serves the web UI over HTTP/HTTPS on your local network
3. **Pair devices** — scan a QR code or enter a four-word mnemonic on any device to authenticate
4. **Code** — pick an AI harness and model, type instructions alongside your code; the agent reads files, proposes edits, and runs shell commands with your approval
5. **Stay in sync** — edits from agents and users are tracked with three-way merge; all paired devices see changes live

Agents propose; you approve. Shell commands and file writes require explicit permission from any paired device before the daemon executes them.

## Requirements

- [Rust](https://rustup.rs/) (Cargo and rustc)
- [Node.js](https://nodejs.org/) 20+ (with npm)
- An ACP-compatible agent CLI (e.g. [Claude Code](https://docs.anthropic.com/claude/docs/claude-code), [Gemini CLI](https://github.com/google-gemini/gemini-cli))

## Setup

**Build** (compiles frontend and embeds it into the Rust binary):

```bash
# Linux / macOS
./build.sh

# Windows (PowerShell)
.\build.ps1

# Or via Make
make build
```

This produces `bin/local_agent` (release build, with the frontend embedded via `rust-embed`).

**Run:**

```bash
./bin/local_agent add-folder /path/to/your/project   # Register a workspace
./bin/local_agent start                               # Start the daemon
./bin/local_agent pair                                # Show QR code + mnemonic for device pairing
```

Open the URL printed by `start` in any browser on your network. Additional devices navigate to the local IP, then use `pair` to authenticate.

## CLI Reference

```
local_agent start                  Start the daemon (foreground or --background)
local_agent stop                   Stop the daemon
local_agent status                 Show daemon status and address

local_agent add-folder <path>      Register a workspace folder
local_agent remove-folder <id>     Unregister a workspace folder
local_agent list-folders           List registered workspaces

local_agent pair                   Generate QR code + mnemonic passcode for a new device
local_agent devices                List paired devices
local_agent revoke <id>            Revoke a device's access

local_agent install-service        Install as a background system service (systemd / launchd / Windows)
local_agent uninstall-service      Remove the system service

local_agent logs                   Show daemon logs
local_agent help                   Show help
```

## Architecture

- **ACP is the only agent integration path** — no per-agent code. The daemon speaks ACP to any compatible agent CLI.
- **Client owns everything** — filesystem access, shell execution, permissions, and session state live in the daemon, not the agent.
- **Agents propose; the client executes** — all file writes and shell commands are gated behind user approval.
- **Devices are thin clients** — the frontend holds no state; everything is derived from events broadcast over WebSocket.

## Security

**Agent security** — agents are untrusted subprocesses:

- **Process isolation** — agents run as separate processes over ACP; they can't touch your filesystem or shell on their own
- **Workspace-scoped** — shell and file access are confined to the registered workspace; path traversal and symlink escapes are rejected
- **Auditable** — every permission decision and meaningful action is recorded (in-memory audit log + SQLite event store)

**Access security** — the daemon is a local network service with single-user, multi-device auth for use on local networks and tailscale:

- **Single User Auth** — Authenticate devices via QR code or four-word mnemonic. 
- **TLS on by default** with auto-generated self-signed cert
- **Rate limits, request size caps, and WS Origin/CSRF checks** on every endpoint

## Key Docs

| File | Purpose |
|---|---|
| [`docs/plans/Blueprint.md`](docs/plans/Blueprint.md) | Architecture and design source of truth |
| [`docs/STATUS.md`](docs/STATUS.md) | Task-level implementation status |
| [`docs/known-issues.md`](docs/known-issues.md) | Known gaps and deferred issues |
| [`AGENTS.md`](AGENTS.md) | Rules and context for AI agents working in this repo |

## License

GNU General Public License v3.0 — see [LICENSE](LICENSE).
