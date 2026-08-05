# AGENTS.md

## Product

Self-hosted, cross-platform web IDE with built-in AI. A Rust daemon serves thin browser clients on the local network; files and state remain on the host. ACP orchestrates external agents alongside a VS Code-style editor.

## App workflow

1. `local_agent add-folder` registers a workspace; `local_agent pair` creates a QR code or mnemonic for device pairing.
2. Paired devices open the web IDE; additional devices authenticate from the lock screen with the mnemonic.
3. The user selects an agent harness and model, then prompts beside the code. The agent streams responses and proposed tool actions to the chat.
4. Approved client-side actions read/write workspace files or run scoped shell commands. Edits sync live; concurrent changes use revision tracking and a three-way merge.
5. On mobile, explorer, editor, chat, and settings are separate bottom-nav panels.

## State and trust

- Configuration is stored in `~/.local-agent`; events use an append-only SQLite WAL store. Clients receive authenticated WebSocket sync, reconnection, and event replay.
- Pairing issues expiring device credentials. Any paired device can answer a pending file-write or shell-command permission prompt.

## File Map

```text
src/                 Rust daemon, library, and CLI (See src/AGENTS.md)
├── acp/             ACP engine (See src/acp/AGENTS.md)
├── api/             REST/WS API (See src/api/AGENTS.md)
├── app/             daemon lifecycle/TLS (See src/app/AGENTS.md)
├── cli/             commands and service installers
├── config/          config model/store
├── interfaces/      wire types and traits
├── pairing/         device pairing/credentials
├── workspace/       workspace/path containment
└── ...              domain services and utilities
web/                 React/Vite client (See web/AGENTS.md)
└── src/
    ├── components/  UI (See web/src/components/AGENTS.md)
    ├── hooks/       state and backend hooks
    ├── lib/         API client and utilities
    └── types/       frontend wire types
tests/               integration/contract tests (See tests/AGENTS.md)
└── contract_runner/ REST/WS runner (See tests/contract_runner/AGENTS.md)
docs/                specs, plans, status, reviews (See docs/AGENTS.md)
└── plans/           epics and stories (See docs/plans/AGENTS.md)
configs/             bundled runtime defaults (See configs/AGENTS.md)
scripts/             setup and smoke utilities (See scripts/AGENTS.md)
```

Build: `build.sh`, `build.ps1`, `Makefile`, `build.rs` → `make check`.

## Architecture

- ACP is the only agent integration boundary; do not add per-agent integrations.
- The client owns filesystem access, shell execution, permissions, and sessions.
- The UI never talks directly to an agent implementation.
- Agents propose actions; the client performs approved actions.
- Prefer small, maintainable code and simple algorithms; avoid duplicated or speculative code.
- Before planning complicated fixes or features, compare 3 or 4 possible implementations.
- Avoid large files; plan modules if the file will grow.

## Development

- Build with `./build.sh` or `.\build.ps1` (Windows); output is `bin/local_agent`(.exe if Windows).
- Run frontend HMR with `make dev` (or `scripts/dev.sh`) and open `http://localhost:5173`. It starts the daemon and Vite, which proxies `/api` and `/ws`. If `web/dist/index.html` is missing after `make clean`, run `cd web && npm run build`. `cargo-watch` is optional; without it, restart the daemon after Rust changes.
- During development, use `make qcheck` to auto-fix formatting/lints and run the full test suite.
- Before handoff, run `make check` for the full gate (Rust fmt/clippy/tests, frontend lint/build, and contracts). Use `make lint` for a faster style pass.
- For Rust-only changes, run `cargo fmt -q --check`, `cargo clippy -q --all-targets -- -D warnings`, and `cargo test -q --all-targets`.
- Frontend tests are pure utility tests in `web/src/lib/`; run `make test-frontend` and avoid React/DOM-coupled tests.
- See `docs/development/building.md` for release and toolchain details. Record unrelated failures in `docs/known-issues.md` instead of expanding scope.
- Keep code small: comment non-obvious or security-sensitive intent, avoid megafiles and overcomplicated tests, and include tests plus a suggested commit message when handing off.

## Plans
List relevant folder to see task status. Review after milestones. Task reviewer deletes tasks after review, or updates status if work is not complete. Add action items from review as stories unless they are immediately fixable. 

**Plan Folder**
```
docs/plans/
├── Blueprint.md  # summary of the app. Ignore for now - needs updating. 
├── status-epic-difficulty.md
├── epic/
│   └── status-story-difficulty.md
└── other_tasks/ # bugs, chores, etc.
    └── status-task-difficulty-urgency.md
```

## Security

This daemon exposes a browser UI and may execute commands or write files.

- Default to TLS.
- Reject workspace symlinks; contain and validate paths.
- Rate-limit unauthenticated endpoints and cap request/response sizes.
- For endpoints or file/command surfaces, identify the caller and worst-case impact before implementation.
- Run a focused auth, input, path, command, secrets, TLS, SQLi, and DoS audit after major feature batches or before releases; save findings in `docs/reviews/<date>/`.
- NEVER commit without user review.

## Frontend

- For UI work, follow `.agents/skills/ui-development/SKILL.md`.
- Use co-located utilities and semantic tokens by default; extract meaningful reusable React components, and use `cva` only for their stable variants.
- Use `cn` for conditional classes. Reserve CSS/`@apply` for global or third-party needs; promote repeated arbitrary values to a token or variant.
- Design mobile-first; use `dark:` or `data-theme`, never JS theme conditionals.