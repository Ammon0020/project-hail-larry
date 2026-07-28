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

## Layout

```
src/
  acp/                # the only agent integration boundary
    core/             # session actor, lifecycle, handlers/, ops, registry
    autodetect/       # harness discovery: claude_code, codex, cursor, devin, vibe
    profile*.rs       # agent profiles + registry
    store.rs          # session persistence
  api/                # REST handlers (embed, mcp, profiles, providers, settings)
  app/                # daemon: listen, tls, rate_limit, logging, process
  cli/                # CLI + per-OS service installers
  config/             # config model + store (~/.local-agent)
  events/             # append-only SQLite event store + publisher + replay
  files/              # client-side file read/write surface
  shell/              # scoped shell execution surface
  permissions/        # permission manager + sink (file-write/shell prompts)
  workspace/          # workspace registration + path containment
  sync/               # WebSocket sync + three-way merge
  pairing/            # QR/mnemonic device pairing + expiring credentials
  interfaces/         # ACP wire types, traits, DTOs shared with web
  mcp/ migrate/       # MCP integration; config migration
  bin/mockagent.rs    # mock ACP agent for tests
web/src/
  components/         # editor/, chat/, settings/, chrome, ui/ (radix primitives)
  hooks/              # useBackend, useChatTabs, useEditorSettings, useTheme, ...
  lib/                # api.ts (REST/WS client), errors, modelPrefs, theme, utils
  types/              # mirror of src/interfaces/
tests/
  contract/           # source-of-truth contract suite (fixtures, golden, scripts)
  contract_runner/    # harness: rest, ws, compare, redactor
docs/
  STATUS.md           # current task status (<100 lines, <90 cols)
  known-issues.md     # deferred gaps
  plans/              # epics + stories (status-prefixed filenames) + Blueprint.md
  specs/              # backend-spec, ui-spec, chat-panel-spec
  reference/          # acp/, mcp/ protocol reference
  reviews/<date>/     # audit findings
configs/              # shipped runtime config (system-messages.json)
scripts/              # setup, spa-smoke, exec-guard
build.sh / build.ps1 / Makefile / build.rs   # build + `make check` gate
```

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
- Before completion, run the unified gate: `make check` (fmt + clippy + cargo
  test + frontend eslint/build + contract suite). For a fast style/correctness
  pass use `make lint` (fmt + clippy + eslint). Individual targets (`make test`,
  `make test-contract`, `make lint-rust`, `make lint-frontend`) remain available.
- For Rust-only changes, `cargo test -q --all-targets`,
  `cargo clippy -q --all-targets -- -D warnings`, and `cargo fmt -q --check`
  suffice; `make check` adds the frontend + contract bar for full CI parity.
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