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

- Use `./build.sh` on Linux/macOS or `.\build.ps1` on Windows → `bin/local_agent`.
- For frontend dev with HMR, use `make dev` (or `scripts/dev.sh`) — starts the
  Rust daemon and Vite dev server together. Open http://localhost:5173;
  Vite proxies `/api` and `/ws` to the daemon. Ctrl+C stops both. Requires
  `web/dist/index.html` (run `cd web && npm run build` once after `make clean`).
- During active development, use `make qcheck` to automatically fix formatting/lints and quietly run the full test suite.
- Before completion, run the verbose unified gate: `make check` (fmt + clippy + cargo
  test + frontend eslint/build + contract suite). For a fast style/correctness
  pass use `make lint` (fmt + clippy + eslint). Individual targets (`make test`,
  `make test-contract`, `make lint-rust`, `make lint-frontend`, `make fix`) remain available.
- For Rust-only changes, `cargo test -q --all-targets`,
  `cargo clippy -q --all-targets -- -D warnings`, and `cargo fmt -q --check`
  suffice; `make check` adds the frontend + contract bar for full CI parity.
- Frontend unit tests (vitest) cover pure utility functions in `web/src/lib/`.
  Run with `make test-frontend` (or `cd web && npm test -- --run`). Tests are
  pure-function only — no React rendering, no DOM mocking. Add tests for new
  pure functions; skip hooks/components that couple to `useBackend` or the DOM.
- Release builds use fat LTO + `codegen-units = 1` for maximum runtime
  performance (see `[profile.release]` in `Cargo.toml`). The first cold release
  compile is slower (~6 min) but produces a faster binary; incremental rebuilds
  are much faster via sccache. On x86_64 Linux, mold is used as the linker
  to offset LTO link time. sccache and mold are local-dev optimizations only —
  the repo's `.cargo/config.toml` is intentionally empty so CI runners without
  these tools build out of the box. `scripts/setup.sh` (and `setup.ps1` for
  sccache) write the optimization config to the user-level `~/.cargo/config.toml`;
  `setup.sh --verify` checks for the tools. `make check` and `make qcheck` use
  the debug profile and are unaffected by LTO.
- Record unrelated test failures in `docs/known-issues.md`; do not expand scope.
- For planning work, follow `.agents/skills/plan-management/SKILL.md`.
- Discover work by listing `docs/plans/`, then the chosen epic folder. Use status-prefixed filenames; rename them when status changes. Keep plans concise and executable in one branch.
- Suggest a commit message and tests when handing work off.
- Add brief comments for non-obvious intent, constraints, and security-sensitive behavior. Do not comment code that is already self-explanatory.
- Avoid megafiles. Break up files when they get too large. 
- Avoid creating tests that overcomplicate the code. If a test is too specific or would make the code harder to maintain, consider if it's really necessary.

## Plans
List relevant folder to see task status. Review after milestones. Task reviewer deletes tasks after review, or updates status if work is not complete. Add action items from review as stories unless they are immediately fixable. 

**Folder hierarchy**
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