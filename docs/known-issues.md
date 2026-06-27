# Known Issues

Gaps and deferred work tracked from review passes. Each entry is a one-line
note so the next agent can pick it up without re-reading the full review file.

## Web frontend — deferred review findings (from `review/2026-06-27/`)

These 8 `web-*` findings were triaged but not fixed in this pass because they
need a larger refactor or a design decision. The review markdown files remain
in `review/2026-06-27/` (not moved to `implemented/`).

- **web-app-side-effect-during-render.md** — `App.tsx` calls
  `backend.loadSessionEvents` inside a render-time state-adjustment block and
  all hooks run after an `if (!paired) return` early return (rules-of-hooks
  violation). Fix: move the fetch into a guarded `useEffect` and move the
  early return below all hook calls. Larger refactor — touches session-restore
  flow.
- **web-chatpanel-agent-model-restore-race.md** — `ChatPanel`'s render-time
  agent/model restoration reads `selectedAgent` before it is validated in the
  same pass. Fix: move the restoration into a `useEffect` keyed on `agents`
  (or compute agent+model together). Coupled to the app-side-effect finding.
- **web-dark-mode-js-class-no-light-theme.md** — `index.css` defines identical
  `:root` and `.dark` palettes (no light theme); `main.tsx` hardcodes the
  `dark` class via JS. Needs a genuine light-palette design decision before
  wiring `data-theme`.
- **web-dead-ui-mobilesettings-theme-toggle.md** — `MobileSettings` theme
  buttons have no handlers. Blocked on the light-theme decision above.
- **web-eslint-disable-exhaustive-deps-blanket.md** — `useBackend.ts` opens
  with a file-level `eslint-disable react-hooks/exhaustive-deps`. Fix: move
  loader functions outside the hook or wrap in `useCallback` with correct deps.
  Larger refactor of the 300-line hook.
- **web-event-log-unbounded-growth.md** — `eventsRef` appends every WebSocket
  event forever; per-session rendering is O(total). Needs a per-session
  `Map` index and/or eviction cap (perf refactor).
- **web-raw-palette-colors-not-semantic-tokens.md** — pervasive raw Tailwind
  palette classes (`text-gray-400`, `bg-gray-800`, etc.) across all
  components instead of semantic tokens. Systematic migration; large surface
  area, do in a dedicated pass.
- **web-shadcn-components-not-used.md** — no shadcn primitives are installed
  (`components/ui/` is absent); selects/modal/dropdown/popover are hand-rolled.
  Migrating to shadcn `Select`/`Dialog`/`DropdownMenu`/`Popover` requires
  installing the components and is a larger refactor.

## Pre-existing lint notes

- `App.tsx` has a pre-existing `react-hooks/rules-of-hooks` error (early
  return before hooks) and an `exhaustive-deps` warning on the keyboard
  shortcut effect. Both predate this pass and are tracked under the
  `web-app-side-effect-during-render` finding above.
