# Known Issues

Gaps and deferred work tracked from review passes. Each entry is a one-line
note so the next agent can pick it up without re-reading the full review file.

## Web frontend — deferred review findings (from `docs/reviews/2026-07-06/`)

RESOLVED (2026-07-06). All 8 previously-deferred `web-*` findings have been
fixed as part of the light-theme + shadcn foundation work — see
`docs/STATUS.md` → Recent Fixes (2026-07-06). Summary of how each was closed:

- **web-app-side-effect-during-render** — `App.tsx` no longer calls
  `loadSessionEvents` during render; it's in a guarded `useEffect` with a
  ref-tracked one-time load, and all hooks run before the `if (!paired)` early
  return.
- **web-chatpanel-agent-model-restore-race** — restoration computes a local
  `nextAgent` and derives the model from it (no stale `selectedAgent` read),
  kept render-time to avoid `set-state-in-effect`.
- **web-dark-mode-js-class-no-light-theme** — real light palette added in
  `:root` (dark in `.dark`); `src/lib/theme.ts` + `useTheme` manage
  `dark | light | system` (default dark), applied by `main.tsx`.
- **web-dead-ui-mobilesettings-theme-toggle** — the theme toggle is wired to
  `useTheme` (Dark/Light/System) with active state.
- **web-eslint-disable-exhaustive-deps-blanket** — file-level disable removed;
  replaced by a single justified targeted disable on the run-once mount effect.
- **web-event-log-unbounded-growth** — in-memory event log bounded at
  `MAX_EVENTS=5000` via a `commitEvents` helper (SQLite remains source of
  truth; `loadSessionEvents` re-fetches evicted history).
- **web-raw-palette-colors-not-semantic-tokens** — all components migrated to
  semantic tokens; intentional status/signal hues (green/red/amber) retained.
- **web-shadcn-components-not-used** — shadcn primitives installed under
  `src/components/ui/` (button, select, dialog, dropdown-menu, popover);
  ChatPanel/SettingsModal now use `Select`/`Dialog`.

## Notes

- Status-dot colors in `ChatHistory` (`bg-gray-600`, `bg-blue-400`) are kept as
  intentional signal colors; they read acceptably in both themes.
- Editor status bar uses fixed `bg-status-bar text-white` (VS Code blue) by
  design in both themes.

## Image upload — Claude Code inline-image delivery gap

The image upload flow (Mode B) is implemented per ACP spec: when an agent
advertises `PromptCapabilities.Image`, the transport sends an inline
`ImageBlock` (base64) with a `Uri` hint; otherwise it sends a
`ResourceLinkBlock` + text instruction telling the agent to read the file.

Claude Code CLI (as of 2.1.128) has a documented parity gap: it does not
reliably deliver inline base64 images to the model even when an `ImageBlock` is
passed in the stdin frame. The resource-link + "please read this file" fallback
path is therefore the robust one for Claude Code today — the agent reads the
file from disk via its own `read` tool (which is the path Claude Code actually
supports, however imperfectly). We mitigate the known `read`-tool bugs (mime
detection by extension, no validation) by validating magic bytes and writing
the correct extension ourselves before storing the upload.

When Claude Code fixes the inline-image gap upstream, no change is needed on
our side — the capability gate will already send the inline `ImageBlock` to any
agent that advertises image support.

## Security audit — deferred findings (from 2026-07-07 audit)

Two findings from the security audit are deferred as design items rather than
code fixes:

- **sec-auth-no-authorization-tiers (Medium):** All paired devices have equal
  privileges — any device can revoke another device or register an arbitrary
  filesystem path as a workspace. Fix requires introducing an authorization
  tier (e.g. first-paired device as admin) or host-side confirmation for
  destructive actions. Tracked as a design decision, not a quick patch.
- **sec-auth-credentials-in-query-params (Low):** Device credentials are passed
  as `deviceId`/`secret` query params on WebSocket/SSE handshakes (browsers
  can't set headers on WS). Acceptable trade-off for direct LAN+TLS, but a
  short-lived single-use WS ticket exchanged via the authenticated REST API
  would eliminate the leakage vector if a reverse proxy is ever placed in
  front.
